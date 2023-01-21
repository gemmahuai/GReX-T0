pub mod args;
pub mod capture;
pub mod common;
pub mod exfil;
pub mod monitoring;
pub mod processing;

use capture::capture_task;
use clap::Parser;
use crossbeam_channel::bounded;
use monitoring::monitor_task;
use processing::downsample_thread;

const THREAD_CHAN_SIZE: usize = 100;

fn main() {
    // Get the CLI options
    let cli = args::Cli::parse();
    // Create the capture
    let cap = capture::Capture::new(&cli.cap_interface, cli.cap_port);
    // Create the payload channel
    let (ps, pr) = bounded(THREAD_CHAN_SIZE);
    // Create the capture statistics channel
    let (ss, sr) = bounded(THREAD_CHAN_SIZE);
    // Create the stokes channel (downsampled)
    let (stokes, stoker) = bounded(THREAD_CHAN_SIZE);
    // Start the processing thread
    let process_thread = std::thread::spawn(move || downsample_thread(pr, stokes, cli.downsample));
    // Start the monitoring thread
    let mon_thread = std::thread::spawn(move || monitor_task(sr));
    // Start the capture thread
    let cap_thread = std::thread::spawn(move || capture_task(cap, ps, ss));
    // Start a dummy exfil
    let dummy = std::thread::spawn(move || exfil::dummy_consumer(stoker));

    // Join the threads into the main task once they bail
    process_thread.join().unwrap();
    mon_thread.join().unwrap();
    cap_thread.join().unwrap();
    dummy.join().unwrap();
}
