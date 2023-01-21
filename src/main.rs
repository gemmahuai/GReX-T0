pub mod args;
pub mod capture;
pub mod exfil;
pub mod monitoring;

use clap::Parser;
use crossbeam_channel::bounded;
use monitoring::monitor_task;

const CHANNEL_SIZE: usize = 100;

fn main() {
    // Get the CLI options
    let cli = args::Cli::parse();
    // Create the capture
    let cap = capture::Capture::new(&cli.cap_interface, cli.cap_port);
    // Create the payload channel
    let (ps, pr) = bounded(CHANNEL_SIZE);
    // Create the capture statistics channel
    let (ss, sr) = bounded(CHANNEL_SIZE);
    // Start the monitoring thread
    let mon_thread = std::thread::spawn(move || monitor_task(sr));
    // Start the capture thread
    let cap_thread = std::thread::spawn(move || cap.capture_task(ps, ss));
    // Start a dummy exfil
    let dummy = std::thread::spawn(move || exfil::dummy_consumer(pr));

    // Join the threads into the main task once they bail
    mon_thread.join().unwrap();
    cap_thread.join().unwrap();
    dummy.join().unwrap();
}
