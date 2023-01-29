//! Inter-thread processing (downsampling, voltage ring buffer, etc)

use crate::common::{Payload, Stokes, CHANNELS};
use crossbeam::channel::{Receiver, Sender};
use log::info;

// About 10s
const MONITOR_SPEC_DOWNSAMPLE_FACTOR: usize = 305_180;

#[allow(clippy::missing_panics_doc)]
#[allow(clippy::cast_precision_loss)]
pub fn downsample_task(
    payload_recv: &Receiver<Payload>,
    stokes_send: &Sender<Stokes>,
    avg_snd: &Sender<[f64; CHANNELS]>,
    downsample_factor: u16,
) -> ! {
    info!("Starting downsample task");
    // Preallocate averaging vector
    let mut stokes_avg = [0f32; CHANNELS];
    let mut stokes_idx = 0usize;
    // Buffers and indices for monitoring
    let mut monitor_avg = [0f64; CHANNELS];
    let mut monitor_idx = 0usize;
    loop {
        let payload = payload_recv.recv().unwrap();
        // Calculate stokes into the averaging buf
        stokes_avg
            .iter_mut()
            .zip(payload.stokes_i())
            .for_each(|(x, y)| *x += y);
        // If we're at the end, we're done with this average
        if stokes_idx == downsample_factor as usize - 1 {
            // Find the average into an f32 (which is lossless)
            stokes_avg
                .iter_mut()
                .for_each(|v| *v /= f32::from(downsample_factor));
            // Then, push this average to the monitoring average buffer
            monitor_avg
                .iter_mut()
                .zip(stokes_avg)
                .for_each(|(x, y)| *x += f64::from(y));
            // If we've added enough monitor samples, average and send out that
            if monitor_idx == MONITOR_SPEC_DOWNSAMPLE_FACTOR - 1 {
                // Find the average into an f32 (which is lossless)
                monitor_avg
                    .iter_mut()
                    .for_each(|v| *v /= MONITOR_SPEC_DOWNSAMPLE_FACTOR as f64);
                // Don't block here
                let _ = avg_snd.try_send(monitor_avg);
                // And zero the averaging buf
                monitor_avg = [0f64; CHANNELS];
            }
            // Increment the monitor index
            monitor_idx = (monitor_idx + 1) % MONITOR_SPEC_DOWNSAMPLE_FACTOR;
            // Send this average to the standard-rate stokes consumer
            // This could back up if the consumer blocks (like with PSRDADA)
            let _ = stokes_send.try_send(stokes_avg);
            // And finally zero the averaging buf
            stokes_avg = [0f32; CHANNELS];
        }
        // Increment the idx
        stokes_idx = (stokes_idx + 1) % downsample_factor as usize;
    }
}
