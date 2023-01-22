#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub use clap::Parser;
pub use crossbeam_channel::bounded;
use grex_t0::{
    args,
    capture::{decode_task, pcap_task, Capture},
    common::AllChans,
    exfil::dummy_consumer,
    monitoring::monitor_task,
    processing::downsample_thread,
    tui::Tui,
};
use log::LevelFilter;
pub use thread_priority::{ThreadBuilder, ThreadPriority};

macro_rules! priority_thread_spawn {
    ($thread_name:literal, $fcall:expr) => {
        ThreadBuilder::default()
            .name($thread_name)
            .priority(ThreadPriority::Crossplatform(95.try_into().unwrap()))
            .spawn(move |result| {
                assert!(result.is_ok());
                $fcall;
            })
            .unwrap()
    };
}

fn main() -> anyhow::Result<()> {
    // Get the CLI options
    let cli = args::Cli::parse();

    // Only log to stdout if we're not tui-ing
    if cli.tui {
        tui_logger::init_logger(LevelFilter::Trace).expect("Couldn't setup the tui logger");
    } else {
        pretty_env_logger::init();
    }

    // Create the capture
    let cap = Capture::new(&cli.cap_interface, cli.cap_port);

    // Create all the channels
    let (packet_snd, packet_rcv) = bounded(10_000);
    let (payload_snd, payload_rcv) = bounded(10_000);
    let (stat_snd, stat_rcv) = bounded(100);
    let (stokes_snd, stokes_rcv) = bounded(100);

    // Create the collection of channels that we can monitor
    let all_chans = AllChans {
        payload: payload_rcv.clone(),
        stat: stat_rcv.clone(),
        stokes: stokes_rcv.clone(),
        packets: packet_rcv.clone(),
    };

    // Start the threads
    let process_thread = priority_thread_spawn!(
        "downsample",
        downsample_thread(&payload_rcv, &stokes_snd, cli.downsample)
    );
    let monitor_thread = priority_thread_spawn!("monitor", monitor_task(&stat_rcv, &all_chans));
    let dummy_thread = priority_thread_spawn!("dummy", dummy_consumer(&stokes_rcv));
    let decode_thread = priority_thread_spawn!("decode", decode_task(&packet_rcv, &payload_snd));
    let capture_thread = priority_thread_spawn!("capture", pcap_task(cap, &packet_snd, &stat_snd));

    // Start the tui maybe (on the main thread)
    if cli.tui {
        Tui::start()?;
    }

    // Join the threads into the main task once they bail
    process_thread.join().unwrap();
    monitor_thread.join().unwrap();
    capture_thread.join().unwrap();
    dummy_thread.join().unwrap();
    decode_thread.join().unwrap();

    Ok(())
}
