//! Inter-thread processing (downsampling, voltage ring buffer, etc)

use crate::common::{Payload, Stokes, CHANNELS};
use crossbeam::channel::{Receiver, Sender};
use log::info;

/// Fake middleman function to test throughput
#[allow(clippy::missing_panics_doc)]
pub fn dummy_downsample(
    payload_recv: &Receiver<Payload>,
    _: &Sender<Stokes>,
    dump_send: &Sender<Payload>,
    _: u16,
) -> ! {
    loop {
        let payload = payload_recv.recv().unwrap();
        // We ensured we have space in the previous loop
        dump_send.send(payload).unwrap();
    }
}

#[allow(clippy::missing_panics_doc)]
pub fn downsample_task(
    payload_recv: &Receiver<Payload>,
    stokes_send: &Sender<Stokes>,
    dump_send: &Sender<Payload>,
    downsample_factor: u16,
) -> ! {
    info!("Starting downsample task");
    // Preallocate averaging vector
    let mut avg = [0f32; CHANNELS];
    let mut idx = 0usize;
    loop {
        let payload = payload_recv.recv().unwrap();
        // The immediately send it to the ringbuffer
        // This will panic on real errors, which we want
        dump_send.send(payload).unwrap();
        // Calculate stokes into the averaging buf
        avg.iter_mut()
            .zip(payload.stokes_i())
            .for_each(|(x, y)| *x += f32::from(y));
        // If we're at the end, we're done
        if idx == downsample_factor as usize - 1 {
            // Find the average into an f32 (which is lossless)
            avg.iter_mut()
                .for_each(|v| *v /= f32::from(downsample_factor));
            stokes_send.send(avg).unwrap();
            // And zero the averaging buf
            avg = [0f32; CHANNELS];
        }
        // Increment the idx
        idx = (idx + 1) % downsample_factor as usize;
    }
}
