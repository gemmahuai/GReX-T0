#![deny(clippy::all)]
#![warn(clippy::pedantic)]

pub mod args;
pub mod capture;
pub mod common;
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

const THREAD_CHAN_SIZE: usize = 1000;

fn main() {
    // Get the CLI options
    let cli = args::Cli::parse();

    // Create the capture
    let cap = capture::Capture::new(&cli.cap_interface, cli.cap_port);

    // Create all the channels
    let (payload_snd, payload_rcv) = bounded(THREAD_CHAN_SIZE);
    let (stat_snd, stat_rcv) = bounded(THREAD_CHAN_SIZE);
    let (stokes_snd, stokes_rcv) = bounded(THREAD_CHAN_SIZE);

    // Create the collection of channels that we can monitor
    let all_chans = AllChans {
        payload: payload_snd.clone(),
        stat: stat_snd.clone(),
        stokes: stokes_snd.clone(),
    };

    // Start the threads
    let process_thread = ThreadBuilder::default()
        .name("Process")
        .priority(ThreadPriority::Crossplatform(20.try_into().unwrap()))
        .spawn(move |result| {
            assert!(result.is_ok());
            downsample_thread(&payload_rcv, &stokes_snd, cli.downsample);
        })
        .unwrap();
    let monitor_thread = ThreadBuilder::default()
        .name("Monitoring")
        .priority(ThreadPriority::Crossplatform(0.try_into().unwrap()))
        .spawn(move |result| {
            assert!(result.is_ok());
            monitor_task(&stat_rcv, &all_chans);
        })
        .unwrap();
    let dummy_thread = ThreadBuilder::default()
        .name("Dummy exfil")
        .priority(ThreadPriority::Crossplatform(0.try_into().unwrap()))
        .spawn(move |result| {
            assert!(result.is_ok());
            exfil::dummy_consumer(&stokes_rcv);
        })
        .unwrap();
    let cap_thread = ThreadBuilder::default()
        .name("Packet capture")
        .priority(ThreadPriority::Crossplatform(0.try_into().unwrap()))
        .spawn(move |result| {
            assert!(result.is_ok());
            pcap_task(cap, &payload_snd, &stat_snd);
        })
        .unwrap();

    // Join the threads into the main task once they bail
    process_thread.join().unwrap();
    monitor_thread.join().unwrap();
    cap_thread.join().unwrap();
    dummy_thread.join().unwrap();
}
