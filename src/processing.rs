//! Inter-thread processing (downsampling, etc)

use crate::common::{Payload, Stokes, CHANNELS};
use log::info;
use std::time::{Duration, Instant};
use thingbuf::mpsc::{Receiver, Sender};

/// How long should the monitor integrations be?
const MONITOR_INTEGRATION: Duration = Duration::from_secs(10);

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
    let mut monitor_avgs = vec![0f32; CHANNELS];
    let mut monitor_iters = 0;
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
            // Add these averages to the monitor averages
            for (mon_chan, avg_chan) in monitor_avgs.iter_mut().zip(&avg_buf) {
                *mon_chan += avg_chan;
                monitor_iters += 1;
            }
            // And reset averaging
            avg_buf = vec![0f32; CHANNELS];
            avg_iters = 0;
            // Check the monitor send condition
            if last_monitor.elapsed() >= MONITOR_INTEGRATION {
                // Get a handle on the monitor sender (but don't block)
                if let Ok(mut send_ref) = monitor.try_send_ref() {
                    // Write averages directly into it
                    for (out_chan, avg_chan) in send_ref.iter_mut().zip(&monitor_avgs) {
                        *out_chan = avg_chan / monitor_iters as f32;
                    }
                }
                // If we actually sent monitor info or not, reset it
                monitor_avgs = vec![0f32; CHANNELS];
                monitor_iters = 0;
                last_monitor = Instant::now();
            }
        }
    }
    Ok(())
}
