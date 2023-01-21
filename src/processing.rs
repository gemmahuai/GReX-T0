use crate::common::{Payload, CHANNELS};
use crossbeam_channel::{Receiver, Sender};

pub type Stokes = [f32; CHANNELS];

pub fn downsample_thread(
    payload_recv: Receiver<Payload>,
    stokes_send: Sender<Stokes>,
    downsample_factor: usize,
) {
    println!("Starting downsample task");
    // Preallocate averaging vector
    let mut avg_buf = vec![[0u16; CHANNELS]; downsample_factor];
    let mut idx = 0usize;
    loop {
        // Grab the next payload
        let payload = payload_recv.recv().unwrap();
        // Calculate stokes into the averaging buf
        avg_buf[idx] = payload.stokes_i();
        // If we're at the end, we're done
        if idx == downsample_factor - 1 {
            // Find the average into an f32 (which is lossless)
            let mut avg = [0f32; CHANNELS];
            for chan in 0..CHANNELS {
                for f in 0..downsample_factor {
                    avg[chan] += avg_buf[f][chan] as f32;
                }
            }
            avg.iter_mut().for_each(|v| *v /= downsample_factor as f32);
            // And send out
        }
        // Increment the idx
        idx = (idx + 1) % downsample_factor;
    }
}
