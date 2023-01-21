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
use crossbeam_channel::bounded;
use monitoring::monitor_task;
use processing::downsample_thread;

const THREAD_CHAN_SIZE: usize = 0;

fn main() {
    // Get the CLI options
    let cli = args::Cli::parse();
    // Create the capture
    let cap = capture::Capture::new(&cli.cap_interface, cli.cap_port);
    // Create the payload channel
    let (payload_snd, payload_rcv) = bounded(THREAD_CHAN_SIZE);
    // Create the capture statistics channel
    let (stat_snd, stat_rcv) = bounded(THREAD_CHAN_SIZE);
    // Create the stokes channel (downsampled)
    let (stokes_snd, stokes_rcv) = bounded(THREAD_CHAN_SIZE);
    // Start the processing thread
    let process_thread =
        std::thread::spawn(move || downsample_thread(&payload_rcv, &stokes_snd, cli.downsample));
    // Start the monitoring thread
    let mon_thread = std::thread::spawn(move || monitor_task(&stat_rcv));
    // Start a dummy exfil
    let dummy = std::thread::spawn(move || exfil::dummy_consumer(&stokes_rcv));
    // And finally kick things off by starting the capture thread
    let cap_thread = std::thread::spawn(move || pcap_task(cap, &payload_snd, &stat_snd));

    // Join the threads into the main task once they bail
    process_thread.join().unwrap();
    mon_thread.join().unwrap();
    cap_thread.join().unwrap();
    dummy.join().unwrap();
}
