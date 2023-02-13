//! Task for injecting a fake pulse into the timestream to test/validate downstream components

use crate::common::{Stokes, CHANNELS};
use anyhow::anyhow;
use byte_slice_cast::AsSliceOf;
use log::info;
use std::time::{Duration, Instant};
use thingbuf::mpsc::blocking::{Receiver, Sender};

/// How often do we inject a fake pulse
const INJECTION_CADENCE: Duration = Duration::from_secs(10);

pub fn pulse_injection_task(input: Receiver<Stokes>, output: Sender<Stokes>) -> anyhow::Result<()> {
    // Read the fake pulse file
    // FIXME - be more clever about the path
    let bytes = std::fs::read("/home/kiran/t0/data/test_frb.dat")?;
    // Create array of floats
    let floats = bytes[..].as_slice_of::<f64>()?;
    let time_samples = floats.len() / CHANNELS;
    info!("Starting pulse injection. Pulse length is {time_samples} samples");

    let mut i = 0;
    let mut currently_injecting = false;
    let mut last_injection = Instant::now();

    loop {
        // Grab stokes from downsample
        let mut s = input.recv().ok_or_else(|| anyhow!("Channel closed"))?;
        if last_injection.elapsed() >= INJECTION_CADENCE {
            last_injection = Instant::now();
            currently_injecting = true;
            i = 0;
        }
        if currently_injecting {
            // Get the window into the fake pulse data
            let this_sample = &floats[i * CHANNELS..(i + 1) * CHANNELS];
            // Add the current time slice of the fake pulse into the stream of real data
            for (i, source) in s.iter_mut().zip(this_sample) {
                *i += *source as f32;
            }
            i += 1;
            // If we've gone through all of it, stop.
            if i == time_samples {
                currently_injecting = false;
            }
        }
        output.send(s)?;
    }
}

#[cfg(test)]
pub mod test {
    use super::*;

    #[test]
    fn test_read_file() {
        let bytes = std::fs::read("data/test_frb.dat").unwrap();
        // Create array of floats
        let floats = bytes[..].as_slice_of::<f64>().unwrap();
        for num in &floats[(2000 * 2048)..(2001 * 2048)] {
            print!("{num:+e} ");
        }
        panic!()
    }
}
