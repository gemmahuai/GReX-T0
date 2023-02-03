//! Inter-thread processing (downsampling, voltage ring buffer, etc)

use crossbeam::channel::{Receiver, Sender};

use crate::common::{Payloads, Stokes, CHANNELS};

#[allow(clippy::missing_panics_doc)]
#[allow(clippy::cast_precision_loss)]
#[must_use]
pub fn downsample(payloads: &Payloads, downsample_power: u32) -> Vec<Stokes> {
    let chunk_size = 2usize.pow(downsample_power);
    payloads
        .chunks(chunk_size) // This should divide evenly if the payloads are in power of 2 chunks
        .map(|pls| {
            let mut stokes_avg = [0f32; CHANNELS];
            for payload in pls {
                if payload.valid {
                    stokes_avg
                        .iter_mut()
                        .zip(payload.stokes_i())
                        .for_each(|(x, y)| *x += y);
                }
            }
            stokes_avg.iter_mut().for_each(|v| *v /= chunk_size as f32);
            stokes_avg
        })
        .collect()
}

#[allow(clippy::missing_panics_doc)]
pub fn downsample_task(
    receiver: &Receiver<Payloads>,
    sender: &Sender<Vec<Stokes>>,
    downsample_power: u32,
) -> ! {
    loop {
        sender
            .send(downsample(&receiver.recv().unwrap(), downsample_power))
            .unwrap();
    }
}
