#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub use clap::Parser;
pub use crossbeam::channel::bounded;
use grex_t0::{
    args::{self, parse_core_range},
    capture::{pcap_task, Capture},
    common::{payload_split, AllChans},
    dumps::{dump_task, trigger_task, DumpRing},
    exfil::dummy_consumer,
    fpga::Device,
    monitoring::monitor_task,
    processing::downsample_task,
};
use log::{info, LevelFilter};
use nix::{
    sched::{sched_setaffinity, CpuSet},
    unistd::Pid,
};
use rsntp::SntpClient;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};

fn main() -> anyhow::Result<()> {
    // Get the CLI options
    let cli = args::Cli::parse();

    // Get the CPU core range
    let mut cpus = parse_core_range(&cli.core_range);

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
    let mut device = Device::new(cli.fpga_addr);
    let packet_start = device.trigger(&time_sync);
    if cli.trig {
        device.force_pps();
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
            std::thread::scope(|s| {
              $(let cpu = cpus.next().unwrap();
                std::thread::Builder::new()
                    .name($thread_name.to_string())
                    .spawn_scoped(s, move || {
                        let mut cpu_set = CpuSet::new();
                        cpu_set.set(cpu).unwrap();
                        sched_setaffinity(Pid::from_raw(0), &cpu_set).unwrap();
                        $fcall;
                    })
                    .unwrap();)+
            });
        };
    }

    thread_spawn! {
        "monitor"    : monitor_task(&stat_rcv, &all_chans, cli.metrics_port),
        "dummy_exfil": dummy_consumer(&stokes_rcv),
        "downsample" : downsample_task(&downsamp_rcv, &stokes_snd, cli.downsample),
        "split"      : payload_split(&payload_rcv, &downsamp_snd, &dump_snd),
        "dump_fill"  : dump_task(dr, &dump_rcv, &signal_rcv, &packet_start),
        "dump_trig"  : trigger_task(&signal_snd, &socket),
        "capture"    : pcap_task(cap, &payload_snd, &stat_snd)
    }

    Ok(())
}
