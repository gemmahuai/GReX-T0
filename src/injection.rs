//! Task for injecting a fake pulse into the timestream to test/validate downstream components

use crate::common::{Stokes, CHANNELS};
use anyhow::anyhow;
use byte_slice_cast::AsSliceOf;
use log::info;
use ndarray::{s, ArrayView};
use std::time::{Duration, Instant};
use std::path::PathBuf;
use thingbuf::mpsc::blocking::{Receiver, Sender};

pub fn pulse_injection_task(
    input: Receiver<Stokes>,
    output: Sender<Stokes>,
    cadence: Duration,
    pulse_path: PathBuf,
) -> anyhow::Result<()> {
    // Read the fake pulse file
    // FIXME - be more clever about the path
    let bytes = std::fs::read("/home/kiran/t0/data/test_frb.dat")?;
    // Create array of floats
    let floats = bytes[..].as_slice_of::<f64>()?;
    let time_samples = floats.len() / CHANNELS;

    let block = ArrayView::from_shape((CHANNELS, time_samples), floats)?;

    info!("Starting pulse injection. Pulse length is {time_samples} samples");

    let mut i = 0;
    let mut currently_injecting = false;
    let mut last_injection = Instant::now();

    loop {
        // Grab stokes from downsample
        let mut s = input.recv().ok_or_else(|| anyhow!("Channel closed"))?;
        if last_injection.elapsed() >= cadence {
            last_injection = Instant::now();
            currently_injecting = true;
            i = 0;
        }
        if currently_injecting {
            // Get the slice of fake pulse data
            let this_sample = block.slice(s![.., i]);
            // Add the current time slice of the fake pulse into the stream of real data
            for (i, source) in s.iter_mut().zip(this_sample) {
                *i += *source as f32 * 10000.0;
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
