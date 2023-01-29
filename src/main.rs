#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub use clap::Parser;
pub use crossbeam::channel::bounded;
use grex_t0::{
    args::{self, Exfil},
    capture::{pcap_task, Capture},
    common::{payload_split, AllChans, CHANNELS},
    dumps::{dump_task, trigger_task, DumpRing},
    exfil::{dada_consumer, dummy_consumer},
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
use psrdada::builder::DadaClientBuilder;
use rsntp::SntpClient;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use tokio::net::TcpListener;

#[tokio::main]
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

    // If we're using PSRDADA, create the buffer,
    // which will be cleaned up on program exit
    let mut dada_buf = None;
    if let Some(Exfil::Psrdada { key, samples }) = cli.exfil {
        info!("Creating PSRDADA buffer");
        // We're sending float32s, so we know how big each window is
        dada_buf = Some(
            DadaClientBuilder::new(key)
                .buf_size((samples * CHANNELS * 4).try_into().unwrap())
                .lock(true)
                .page(true)
                .build()
                .unwrap(),
        );
    }

    // Create the capture
    let cap = Capture::new(&cli.cap_interface, cli.cap_port);

    // Payloads from capture to split
    let (payload_snd, payload_rcv) = bounded(10_000);
    // Payloads from split to dump ring and downsample
    let (downsamp_snd, downsamp_rcv) = bounded(10_000);
    let (dump_snd, dump_rcv) = bounded(10_000);
    // Stats
    let (stat_snd, stat_rcv) = bounded(100);
    // Stokes out from downsample
    let (stokes_snd, stokes_rcv) = bounded(100);
    // Triggers for dumping ring
    let (signal_snd, signal_rcv) = bounded(100);
    // Average stokes data for monitoring
    let (avg_stokes_snd, avg_stokes_rcv) = bounded(100);

    // Create the collection of channels that we can monitor
    let all_chans = AllChans {
        stokes: stokes_rcv.clone(),
        payload_to_downsample: downsamp_rcv.clone(),
        payload_to_ring: dump_rcv.clone(),
        cap_payload: payload_rcv.clone(),
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
        "monitor"    : monitor_task(&stat_rcv, &avg_stokes_rcv, &all_chans, &device),
        "dummy_exfil": if let Some(Exfil::Psrdada { samples, key }) = cli.exfil {
                            dada_consumer(key, &stokes_rcv, &packet_start, samples);
                       } else {
                            dummy_consumer(&stokes_rcv, &avg_stokes_snd);
                       },
        "downsample" : downsample_task(&downsamp_rcv, &stokes_snd, cli.downsample),
        "split"      : payload_split(&payload_rcv, &downsamp_snd, &dump_snd),
        "dump_fill"  : dump_task(dr, &dump_rcv, &signal_rcv, &packet_start),
        "dump_trig"  : trigger_task(&signal_snd, &socket),
        "capture"    : pcap_task(cap, &payload_snd, &stat_snd)
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
    // And clean up the psrdada buffer, if we created it
    if let Some(buf) = dada_buf {
        drop(buf);
    }
    Ok(())
}
