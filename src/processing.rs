//! Inter-thread processing (downsampling, voltage ring buffer, etc)

use crate::common::{Payload, Stokes, CHANNELS};
use crossbeam_channel::{Receiver, Sender};
use log::info;

#[allow(clippy::missing_panics_doc)]
pub fn dummy_downsample(
    payload_recv: &Receiver<Payload>,
    _: &Sender<Stokes>,
    dump_send: &Sender<Payload>,
    _: u16,
) -> ! {
    loop {
        // Busy wait on the next payload
        let payload = loop {
            match payload_recv.try_recv() {
                Ok(v) => break v,
                Err(_) => continue,
            };
        };
        // And send the raw payload to the dumping ringbuffer
        while dump_send.is_full() {}
        // We ensured we have space in the previous loop
        dump_send.try_send(payload).unwrap();
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
        // Busy wait on the next payload
        let payload = loop {
            match payload_recv.try_recv() {
                Ok(v) => break v,
                Err(_) => continue,
            };
        };
        // The immediately send it to the ringbuffer
        while dump_send.is_full() {}
        // This will panic on real errors, which we want
        dump_send.try_send(payload.clone()).unwrap();
        // Calculate stokes into the averaging buf
        avg.iter_mut()
            .zip(payload.stokes_i())
            .for_each(|(x, y)| *x += f32::from(y));
        // If we're at the end, we're done
        if idx == downsample_factor as usize - 1 {
            // Find the average into an f32 (which is lossless)
            avg.iter_mut()
                .for_each(|v| *v /= f32::from(downsample_factor));
            // And busy wait send out
            while stokes_send.is_full() {}
            stokes_send.try_send(avg).unwrap();
            // And zero the averaging buf
            avg = [0f32; CHANNELS];
        }
        // Increment the idx
        idx = (idx + 1) % downsample_factor as usize;
    }
}
