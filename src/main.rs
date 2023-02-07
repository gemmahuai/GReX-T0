#![deny(clippy::all)]
//#![warn(clippy::pedantic)]

use anyhow::bail;
pub use clap::Parser;
use core_affinity::CoreId;
use grex_t0::{
    args, capture,
    dumps::{self, DumpRing},
    exfil,
    fpga::Device,
    monitoring, processing,
};
use log::{info, LevelFilter};
use rsntp::SntpClient;
use thingbuf::mpsc::{channel, with_recycle};
use tokio::{runtime, try_join};

fn main() -> anyhow::Result<()> {
    // Enable tokio console
    console_subscriber::init();
    // Get the CLI options
    let cli = args::Cli::parse();
    // Logger init
    pretty_env_logger::formatted_builder()
        .filter_level(LevelFilter::Info)
        .init();
    // Setup NTP
    info!("Synchronizing time with NTP");
    let client = SntpClient::new();
    let time_sync = client.synchronize(cli.ntp_addr).unwrap();
    // Setup the FPGA
    let mut device = Device::new(cli.fpga_addr, cli.requant_gain);
    let packet_start = device.trigger(&time_sync);
    if cli.trig {
        device.force_pps();
    }
    // Create a dedicated single-threaded async runtime for the capture task
    let (pb_s, pb_r) = with_recycle(1024, capture::PayloadRecycle::new());

    // Create the dump ring
    let ring = DumpRing::new(cli.vbuf_power);

    // Create channels to connect everything else
    let (ds_s, ds_r) = channel(32768);
    let (ex_s, ex_r) = channel(100);
    let (dump_s, dump_r) = channel(100);
    let (trig_s, trig_r) = channel(5);
    let (stat_s, stat_r) = channel(100);
    let (avg_s, avg_r) = channel(100);

    // Create a runtime for some of the less critical tasks
    let monitoring_tasks = std::thread::Builder::new()
        .name("monitoring".to_string())
        .spawn(move || -> anyhow::Result<()> {
            if !core_affinity::set_for_current(CoreId { id: 10 }) {
                bail!("Couldn't set core affinity on capture thread");
            }
            let rt = runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(async {
                let (_, _, _) = try_join!(
                    // Monitoring
                    tokio::task::Builder::new()
                        .name("monitor_collect")
                        .spawn(monitoring::monitor_task(device, stat_r, avg_r))?,
                    tokio::task::Builder::new()
                        .name("web_server")
                        .spawn(monitoring::start_web_server(cli.metrics_port))?,
                    // Dumps
                    tokio::task::Builder::new()
                        .name("dump_trigger")
                        .spawn(dumps::trigger_task(trig_s, cli.trig_port))?,
                )?;
                Ok(())
            })
        })?;

    // Spawn the more timing critical tasks
    let decode_downsample = std::thread::Builder::new()
        .name("decode_split_exfil".to_string())
        .spawn(move || -> anyhow::Result<()> {
            if !core_affinity::set_for_current(CoreId { id: 9 }) {
                bail!("Couldn't set core affinity on capture thread");
            }
            let rt = runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(async {
                let (_, _) = try_join!(
                    // Decode split
                    tokio::task::Builder::new()
                        .name("decode_split")
                        .spawn(capture::decode_split_task(pb_r, ds_s, dump_s))?,
                    // Exfil
                    tokio::task::Builder::new()
                        .name("exfil")
                        .spawn(exfil::dummy_consumer(ex_r))?,
                )?;
                Ok(())
            })
        })?;

    let dump_exfil = std::thread::Builder::new()
        .name("downsample_dump".to_string())
        .spawn(move || -> anyhow::Result<()> {
            if !core_affinity::set_for_current(CoreId { id: 10 }) {
                bail!("Couldn't set core affinity on capture thread");
            }
            let rt = runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(async {
                let (_, _) = try_join!(
                    // Dumps
                    tokio::task::Builder::new()
                        .name("dump_fill")
                        .spawn(dumps::dump_task(ring, dump_r, trig_r, packet_start,))?,
                    // Downsample
                    tokio::task::Builder::new().name("downsample").spawn(
                        processing::downsample_task(ds_r, ex_s, avg_s, cli.downsample_power)
                    )?,
                )?;
                Ok(())
            })
        })?;

    // And then finally the capture task
    let capture_task = std::thread::Builder::new()
        .name("capture".to_string())
        .spawn(move || -> anyhow::Result<()> {
            if !core_affinity::set_for_current(CoreId { id: 8 }) {
                bail!("Couldn't set core affinity on capture thread");
            }
            let rt = runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(async { capture::cap_task(cli.cap_port, pb_s, stat_s).await })
        })?;

    monitoring_tasks.join().unwrap().unwrap();
    decode_downsample.join().unwrap().unwrap();
    capture_task.join().unwrap().unwrap();
    dump_exfil.join().unwrap().unwrap();

    Ok(())
}
