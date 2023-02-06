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
use tokio::runtime;

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
    let (pb_s, pb_r) = with_recycle(32768, capture::PayloadRecycle::new());

    // Bind this thread to a core
    if !core_affinity::set_for_current(CoreId { id: 9 }) {
        bail!("Couldn't set core affinity on capture thread");
    }
    // Create a runtime for all the tasks
    let tasks = std::thread::spawn(move || -> anyhow::Result<()> {
        let rt = runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        rt.block_on(async {
            // Create channels to connect everything
            let (ds_s, ds_r) = channel(100);
            let (ex_s, ex_r) = channel(100);
            let (d_s, d_r) = channel(100);
            let (s_s, s_r) = channel(5);
            // Decode split
            tokio::spawn(async move { capture::decode_split_task(pb_r, ds_s, d_s).await });
            // Downsample
            tokio::spawn(async move {
                processing::downsample_task(ds_r, ex_s, cli.downsample_power).await
            });
            // Exfil
            tokio::spawn(async move { exfil::dummy_consumer(ex_r).await });
            // Dumps
            tokio::spawn(
                async move { dumps::dump_task(d_r, s_r, packet_start, cli.vbuf_power).await },
            );
            tokio::spawn(async move { dumps::trigger_task(s_s, cli.trig_port).await });
            // Monitoring
            tokio::spawn(async move { monitoring::monitor_task(device).await });
            tokio::spawn(async move { monitoring::start_web_server(cli.metrics_port).await });
            Ok(())
        })
    });

    std::thread::spawn(move || -> anyhow::Result<()> {
        // Bind this thread to a core
        if !core_affinity::set_for_current(CoreId { id: 8 }) {
            bail!("Couldn't set core affinity on capture thread");
        }
        let rt = runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        rt.block_on(async { capture::cap_task(cli.cap_port, &pb_s).await })?;
        Ok(())
    })
    .join()
    .unwrap()
    .unwrap();

    tasks.join().unwrap().unwrap();

    Ok(())
}
