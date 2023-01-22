#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub mod args;
pub mod capture;
pub mod common;
pub mod dumps;
pub mod exfil;
pub mod monitoring;
pub mod processing;

use capture::pcap_task;
use clap::Parser;
use common::AllChans;
use crossbeam_channel::bounded;
use monitoring::monitor_task;
use processing::downsample_thread;
use thread_priority::{ThreadBuilder, ThreadPriority};

use crate::capture::decode_task;

const THREAD_CHAN_SIZE: usize = 1000;

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

fn main() {
    // Get the CLI options
    let cli = args::Cli::parse();

    // Create the capture
    let cap = capture::Capture::new(&cli.cap_interface, cli.cap_port);

    // Create all the channels
    let (packet_snd, packet_rcv) = bounded(THREAD_CHAN_SIZE);
    let (payload_snd, payload_rcv) = bounded(THREAD_CHAN_SIZE);
    let (stat_snd, stat_rcv) = bounded(THREAD_CHAN_SIZE);
    let (stokes_snd, stokes_rcv) = bounded(THREAD_CHAN_SIZE);

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
    let dummy_thread = priority_thread_spawn!("dummy", exfil::dummy_consumer(&stokes_rcv));
    let decode_thread = priority_thread_spawn!("decode", decode_task(&packet_rcv, &payload_snd));
    let capture_thread = priority_thread_spawn!("capture", pcap_task(cap, &packet_snd, &stat_snd));

    // Join the threads into the main task once they bail
    process_thread.join().unwrap();
    monitor_thread.join().unwrap();
    capture_thread.join().unwrap();
    dummy_thread.join().unwrap();
    decode_thread.join().unwrap();
}
