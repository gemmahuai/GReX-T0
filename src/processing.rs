//! Inter-thread processing (downsampling, voltage ring buffer, etc)

use crate::common::{self, Payloads, Stokes, CHANNELS};
use crossbeam::channel::{Receiver, Sender};
use itertools::Itertools;
use std::borrow::Borrow;

pub fn avg_stokes_iter<I, T, F>(stokes_iter: I) -> Stokes
where
    F: Borrow<f32>,
    T: IntoIterator<Item = F>,
    I: Iterator<Item = T>,
{
    let mut stokes_avg = [0f32; CHANNELS];
    let mut count = 0f32;
    // Sum
    stokes_iter.for_each(|s| {
        count += 1.0;
        stokes_avg
            .iter_mut()
            .zip(s)
            .for_each(|(x, y)| *x += y.borrow());
    });
    // Div by N
    stokes_avg.iter_mut().for_each(|v| *v /= count);
    stokes_avg
}

#[allow(clippy::missing_panics_doc)]
#[allow(clippy::cast_precision_loss)]
#[must_use]
pub fn downsample(payloads: &Payloads, downsample_power: u32) -> Vec<Stokes> {
    let chunk_size = 2usize.pow(downsample_power);
    // This should divide evenly if the payloads are in power of 2 chunks
    // It's currently set to `capture::PACKETS_PER_CAPTURE` (512)
    payloads
        .iter()
        // Create stokes I for each one
        .map(common::Payload::stokes_i)
        // Chunk by the downsampling amount
        .chunks(chunk_size)
        .into_iter()
        // Average each window
        .map(avg_stokes_iter)
        .collect()
}

#[allow(clippy::missing_panics_doc)]
pub fn downsample_task(
    receiver: &Receiver<Payloads>,
    sender: &Sender<Vec<Stokes>>,
    monitor_sender: &Sender<Stokes>,
    downsample_power: u32,
) -> ! {
    loop {
        let payloads = receiver.recv().unwrap();
        let exfil_downsample = downsample(&payloads, downsample_power);
        let monitor_downsample = avg_stokes_iter(exfil_downsample.iter());
        sender.send(exfil_downsample).unwrap();
        // Send but don't block to monitor
        let _ = monitor_sender.try_send(monitor_downsample);
    }
}
