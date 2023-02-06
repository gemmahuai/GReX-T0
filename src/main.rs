#![deny(clippy::all)]
//#![warn(clippy::pedantic)]

use anyhow::bail;
pub use clap::Parser;
use core_affinity::CoreId;
use grex_t0::{
    args,
    capture::{self},
    dumps::{self},
    exfil::{self},
    fpga::Device,
    monitoring::{self},
    processing::{self},
};
use log::{info, LevelFilter};
use rsntp::SntpClient;
use thingbuf::mpsc::{channel, with_recycle};
use tokio::{join, runtime};

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

    // Create channels to connect everything else
    let (ds_s, ds_r) = channel(32768);
    let (ex_s, ex_r) = channel(100);
    let (d_s, d_r) = channel(100);
    let (s_s, s_r) = channel(5);

    // Create a runtime for some of the less critical tasks
    let monitoring_tasks = std::thread::Builder::new()
        .name("monitoring".to_string())
        .spawn(move || -> anyhow::Result<()> {
            let rt = runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(async {
                if !core_affinity::set_for_current(CoreId { id: 10 }) {
                    bail!("Couldn't set core affinity on capture thread");
                }
                join!(
                    // Monitoring
                    tokio::task::Builder::new()
                        .name("monitor_collect")
                        .spawn(monitoring::monitor_task(device))?,
                    tokio::task::Builder::new()
                        .name("web_server")
                        .spawn(monitoring::start_web_server(cli.metrics_port))?,
                    // Dumps
                    tokio::task::Builder::new()
                        .name("dump_trigger")
                        .spawn(dumps::trigger_task(s_s, cli.trig_port))?,
                );
                Ok(())
            })
        })?;

    // Spawn the more timing critical tasks
    let critical_tasks = std::thread::Builder::new()
        .name("processing".to_string())
        .spawn(move || -> anyhow::Result<()> {
            let rt = runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(async {
                if !core_affinity::set_for_current(CoreId { id: 9 }) {
                    bail!("Couldn't set core affinity on capture thread");
                }
                join!(
                    // Decode split
                    tokio::task::Builder::new()
                        .name("decode_split")
                        .spawn(capture::decode_split_task(pb_r, ds_s, d_s))?,
                    // Downsample
                    tokio::task::Builder::new().name("downsample").spawn(
                        processing::downsample_task(ds_r, ex_s, cli.downsample_power)
                    )?,
                    // Dumps
                    tokio::task::Builder::new()
                        .name("dump_fill")
                        .spawn(dumps::dump_task(d_r, s_r, packet_start, cli.vbuf_power))?,
                    // Exfil
                    tokio::task::Builder::new()
                        .name("exfil")
                        .spawn(exfil::dummy_consumer(ex_r))?
                );
                Ok(())
            })
        })?;

    let capture_task = std::thread::Builder::new()
        .name("capture".to_string())
        .spawn(move || -> anyhow::Result<()> {
            if !core_affinity::set_for_current(CoreId { id: 8 }) {
                bail!("Couldn't set core affinity on capture thread");
            }
            let rt = runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(async { capture::cap_task(cli.cap_port, &pb_s).await })?;
            Ok(())
        })?;

    monitoring_tasks.join().unwrap().unwrap();
    critical_tasks.join().unwrap().unwrap();
    capture_task.join().unwrap().unwrap();

    Ok(())
}
