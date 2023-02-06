//! Inter-thread processing (downsampling, etc)

use crate::common::{Payload, Stokes, CHANNELS};
use log::info;
use std::time::{Duration, Instant};
use thingbuf::mpsc::{Receiver, Sender};

/// How long should the monitor integrations be?
const MONITOR_CADENCE: Duration = Duration::from_secs(10);

#[allow(clippy::missing_panics_doc)]
pub async fn downsample_task(
    receiver: Receiver<Payload>,
    sender: Sender<Stokes>,
    monitor: Sender<Stokes>,
    downsample_power: u32,
) -> anyhow::Result<()> {
    info!("Starting downsample");
    let mut avg_buf = vec![0f32; CHANNELS];
    let mut avg_iters = 0;
    let iters = 2usize.pow(downsample_power);

    let mut last_monitor = Instant::now();

    while let Some(payload) = receiver.recv_ref().await {
        // Compute Stokes I
        let stokes = payload.stokes_i();
        // Add to averaging buf
        avg_buf.iter_mut().zip(stokes).for_each(|(x, y)| *x += y);
        avg_iters += 1;
        if avg_iters == iters {
            // Get a handle on the sender
            let mut send_ref = sender.send_ref().await?;
            // Write averages directly into it
            for (out_chan, avg_chan) in send_ref.iter_mut().zip(&avg_buf) {
                *out_chan = avg_chan / iters as f32;
            }
            // Check the monitor send condition
            if last_monitor.elapsed() >= MONITOR_CADENCE {
                // Get a handle on the monitor sender (but don't block)
                if let Ok(mut mon_ref) = monitor.try_send_ref() {
                    // Write averages from the sender directly into it
                    for (out_chan, avgs) in mon_ref.iter_mut().zip(&*send_ref) {
                        *out_chan = *avgs;
                    }
                }
                last_monitor = Instant::now();
            }
            // And reset averaging
            avg_buf = vec![0f32; CHANNELS];
            avg_iters = 0;
        }
    }
    Ok(())
}
