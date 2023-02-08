use std::time::Duration;

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
use thingbuf::{
    mpsc::{with_recycle, Receiver, Sender},
    Recycle,
};
use tokio::{runtime, try_join};

fn warm_channel<T, R>(capacity: usize) -> (Sender<T, R>, Receiver<T, R>)
where
    T: Clone,
    R: Recycle<T> + Default,
{
    let (s, r) = with_recycle(capacity, R::default());
    for _ in 0..capacity {
        let _ = s.try_send_ref();
        let _ = r.try_recv_ref();
    }
    (s, r)
}

macro_rules! thread_tasks {
    (($thread_name:literal,$core:literal),($(($task_name:literal,$invocation:expr)),+)) => {
        std::thread::Builder::new()
        .name($thread_name.to_string())
        .spawn(move || {
            if !core_affinity::set_for_current(CoreId { id: $core }) {
                bail!("Couldn't set core affinity on thread {}", $thread_name);
            }
            Ok(runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(async move {
                    tokio::task::LocalSet::new()
                        .run_until(async move {
                            let _ = try_join!($(tokio::task::Builder::new()
                                .name($task_name)
                                .spawn_local($invocation)
                                .unwrap(),)+)
                            .unwrap();
                        })
                        .await;
                }))
        })?
    };
    (($thread_name:literal,$core:literal),$invocation:expr) => {
        std::thread::Builder::new()
        .name($thread_name.to_string())
        .spawn(move || {
            if !core_affinity::set_for_current(CoreId { id: $core }) {
                bail!("Couldn't set core affinity on thread {}", $thread_name);
            }
            Ok(runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(async move {
                    tokio::task::LocalSet::new()
                        .run_until(async move {
                            let _ = try_join!(tokio::task::Builder::new()
                                .name($thread_name)
                                .spawn_local($invocation)
                                .unwrap())
                            .unwrap();
                        })
                        .await;
                }))
        })?
    };
}

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

    // Create the dump ring
    let ring = DumpRing::new(cli.vbuf_power);
    // Create channels to connect everything else
    let (pb_s, pb_r) = warm_channel(16384);
    let (ds_s, ds_r) = warm_channel(16384);
    let (ex_s, ex_r) = warm_channel(100);
    let (dump_s, dump_r) = warm_channel(16384);
    let (trig_s, trig_r) = warm_channel(5);
    let (stat_s, stat_r) = warm_channel(100);
    let (avg_s, avg_r) = warm_channel(100);

    // Create a runtime for some of the less critical tasks
    let monitoring_tasks = thread_tasks!(
        ("monitoring", 8),
        (
            ("collect", monitoring::monitor_task(device, stat_r, avg_r)),
            ("web_server", monitoring::start_web_server(cli.metrics_port)),
            ("dump_trig", dumps::trigger_task(trig_s, cli.trig_port))
        )
    );

    // Exfil needs to be its own thread, because PSRDADA only has a blocking API, for god knows why
    // Create a clone of the packet start time to hand off to the other thread
    let psc = packet_start;
    let exfil = match cli.exfil {
        Some(e) => match e {
            args::Exfil::Psrdada { key, samples } => thread_tasks!(
                ("psrdada_exfil", 9),
                exfil::dada_consumer(key, ex_r, psc, 2usize.pow(cli.downsample_power), samples)
            ),
            args::Exfil::Filterbank => todo!(),
        },
        None => thread_tasks!(("dummy_exfil", 9), exfil::dummy_consumer(ex_r)),
    };

    // Spawn the more timing critical tasks
    let decode_downsample = thread_tasks!(
        ("decode_split", 10),
        capture::decode_split_task(pb_r, ds_s, dump_s)
    );

    let downsample = thread_tasks!(
        ("downsamp_dump", 11),
        (
            (
                "dump_fill",
                dumps::dump_task(ring, dump_r, trig_r, packet_start)
            ),
            (
                "downsample",
                processing::downsample_task(ds_r, ex_s, avg_s, cli.downsample_power)
            )
        )
    );

    // Something wonky is still hapening on startup where the capture task locks up, I'll just add some delay
    std::thread::sleep(Duration::from_secs(5));

    // And then finally the capture task
    let capture_task = thread_tasks!(
        ("capture", 12),
        capture::cap_task(cli.cap_port, pb_s, stat_s)
    );

    exfil.join().unwrap().unwrap();
    monitoring_tasks.join().unwrap().unwrap();
    decode_downsample.join().unwrap().unwrap();
    capture_task.join().unwrap().unwrap();
    downsample.join().unwrap().unwrap();

    Ok(())
}
