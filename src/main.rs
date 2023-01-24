#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub use clap::Parser;
pub use crossbeam::channel::bounded;
use crossbeam::thread;
use grex_t0::{
    args,
    capture::{pcap_task, Capture},
    common::{split_task, AllChans},
    dumps::{dump_task, trigger_task, DumpRing},
    exfil::dummy_consumer,
    monitoring::monitor_task,
    processing::downsample_task,
    tui::Tui,
};
use log::LevelFilter;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};

macro_rules! thread_spawn {
    {$($thread_name:literal: $fcall:expr), +} => {
        thread::scope(|s| {
            $(s.builder().name($thread_name.to_string()).spawn(|_| $fcall).unwrap();)+
        }).unwrap();
    };
}

fn main() -> anyhow::Result<()> {
    // Get the CLI options
    let cli = args::Cli::parse();

    // Only log to stdout if we're not tui-ing
    if cli.tui {
        tui_logger::init_logger(LevelFilter::Trace).expect("Couldn't setup the tui logger");
    } else {
        pretty_env_logger::formatted_builder()
            .filter_level(LevelFilter::Info)
            .init();
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
    thread_spawn! {
        "monitor": monitor_task(&stat_rcv, &all_chans),
        "dummy_exfil": dummy_consumer(&stokes_rcv),
        "downsample": downsample_task(&downsamp_rcv, &stokes_snd, cli.downsample),
        "split": split_task(&payload_rcv, &downsamp_snd, &dump_snd),
        "dump_fill": dump_task(dr, &dump_rcv, &signal_rcv),
        "dump_trig": trigger_task(&signal_snd, &socket),
        "capture": pcap_task(cap, &payload_snd, &stat_snd)
    }

    // Start the tui maybe (on the main thread)
    if cli.tui {
        Tui::start()?;
    }

    Ok(())
}
