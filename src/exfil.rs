use crate::common::{Stokes, CHANNELS};
use crossbeam::channel::{Receiver, Sender};

// FIXME (10s)
const MONITOR_SPEC_DOWNSAMPLE_FACTOR: usize = 305_180;

#[allow(clippy::missing_panics_doc)]
#[allow(clippy::cast_precision_loss)]
pub fn dummy_consumer(receiver: &Receiver<Stokes>, avg_snd: &Sender<[f64; CHANNELS]>) {
    let mut avg = [0f64; CHANNELS];
    let mut idx = 0usize;
    loop {
        let stokes = receiver.recv().unwrap();
        // Copy stokes into average buf
        avg.iter_mut()
            .zip(stokes)
            .for_each(|(x, y)| *x += f64::from(y));
        // If we're at the end, we're done
        if idx == MONITOR_SPEC_DOWNSAMPLE_FACTOR - 1 {
            // Find the average into an f32 (which is lossless)
            avg.iter_mut()
                .for_each(|v| *v /= MONITOR_SPEC_DOWNSAMPLE_FACTOR as f64);
            // Don't block here
            let _ = avg_snd.try_send(avg);
            // And zero the averaging buf
            avg = [0f64; CHANNELS];
        }
        // Increment the idx
        idx = (idx + 1) % MONITOR_SPEC_DOWNSAMPLE_FACTOR;
    }
}
