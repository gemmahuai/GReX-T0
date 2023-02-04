#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub use clap::Parser;
pub use crossbeam::channel::bounded;
use grex_t0::{
    args,
    capture::{cap_task, sort_split_task},
    common::AllChans,
    dumps::{dump_task, trigger_task, DumpRing},
    exfil::dummy_consumer,
    fpga::Device,
    monitoring::{metrics, monitor_task},
    processing::downsample_task,
};
use hyper::{server::conn::http1, service::service_fn};
use log::{error, info, LevelFilter};
use nix::{
    sched::{sched_setaffinity, CpuSet},
    unistd::Pid,
};
use rsntp::SntpClient;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use tokio::net::TcpListener;

#[tokio::main(flavor = "current_thread")]
#[allow(clippy::too_many_lines)]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Get the CLI options
    let cli = args::Cli::parse();

    // Get the CPU core range
    let mut cpus = cli.core_range;

    // Set this thread to the first core on the NUMA node so our memory is in the right place
    let mut cpu_set = CpuSet::new();
    cpu_set.set(cpus.next().unwrap()).unwrap();

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
    // Payloads from cap to sort split
    let (cap_snd, ss_rcv) = bounded(100);
    // Payloads from sort split to downsample
    let (ss_snd, downsamp_rcv) = bounded(100);
    // Payloads from sort split to dumps
    let (ss_dump_snd, dumps_rcv) = bounded(100);
    // Stokes from downsample to exfil
    let (downsamp_snd, exfil_rcv) = bounded(100);
    // Big averaged stokes from downsample to monitoring
    let (downsamp_mon_snd, stokes_mon_rcv) = bounded(100);
    // Triggers for dumping ring
    let (signal_snd, signal_rcv) = bounded(100);
    // Capture statistics for monitoring
    let (cap_stat_snd, monitor_stat_rcv) = bounded(100);

    // Create the collection of channels that we can monitor
    let all_chans = AllChans {
        to_exfil: exfil_rcv.clone(),
        to_downsample: downsamp_rcv.clone(),
        to_dump: dumps_rcv.clone(),
        to_sort: ss_rcv.clone(),
    };

    // Create the ring buffer to store voltage dumps
    let dr = DumpRing::new(2usize.pow(cli.vbuf_power));

    // Setup the UDP port for dump triggers
    let socket = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), cli.trig_port))?;
    socket.set_nonblocking(true)?;

    // Start the threads
    macro_rules! thread_spawn {
        {$($thread_name:literal: $fcall:expr), +} => {
              vec![$({let cpu = cpus.next().unwrap();
                std::thread::Builder::new()
                    .name($thread_name.to_string())
                    .spawn( move || {
                        let mut cpu_set = CpuSet::new();
                        cpu_set.set(cpu).unwrap();
                        sched_setaffinity(Pid::from_raw(0), &cpu_set).unwrap();
                        $fcall;
                    })
                    .unwrap()}),+]
        };
    }

    let handles = thread_spawn! {
        "monitor"    : monitor_task(&all_chans, &device, &stokes_mon_rcv, &monitor_stat_rcv),
        "dummy_exfil": dummy_consumer(&exfil_rcv),
        "dump_fill"  : dump_task(dr, &dumps_rcv, &signal_rcv, &packet_start),
        "dump_trig"  : trigger_task(&signal_snd, &socket),
        "downsample" : downsample_task(&downsamp_rcv, &downsamp_snd, &downsamp_mon_snd, cli.downsample_power),
        "sort_split" : sort_split_task(&ss_rcv, &ss_snd, &ss_dump_snd, &cap_stat_snd),
        "capture"    : cap_task(cli.cap_port, &cap_snd)
    };

    // And then finally spin up the webserver for metrics on the main thread
    info!("Starting metrics webserver");
    let addr = SocketAddr::from(([0, 0, 0, 0], cli.metrics_port));
    let listener = TcpListener::bind(addr).await?;
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            Err(e) => {
                error!("TCP accept error: {}", e);
                break;
            }
        };
        tokio::task::spawn(async move {
            if let Err(err) = http1::Builder::new()
                .serve_connection(stream, service_fn(metrics))
                .await
            {
                println!("Error serving connection: {err:?}");
            }
        });
    }
    // Join them all when we kill the task
    for handle in handles {
        handle.join().unwrap();
    }
    Ok(())
}
