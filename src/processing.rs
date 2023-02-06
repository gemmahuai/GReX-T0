//! Inter-thread processing (downsampling, etc)

use crate::common::{Payload, Stokes, CHANNELS};
use log::info;
use std::time::{Duration, Instant};
use thingbuf::mpsc::{Receiver, Sender};

/// How many packets before we send one off to monitor
const MONITOR_CADENCE: Duration = Duration::from_secs(10);

#[allow(clippy::missing_panics_doc)]
pub async fn downsample_task(
    receiver: Receiver<Payload>,
    sender: Sender<Stokes>,
    monitor: Sender<Stokes>,
    downsample_power: u32,
) -> anyhow::Result<()> {
    info!("Starting downsample");

    // We have two averaging states, one for the normal downsample process and one for monitoring
    // They differ in that the standard "thru" connection is averaging by counts and the monitoing one is averaging by time
    let downsamp_iters = 2usize.pow(downsample_power);
    let mut downsamp_buf = vec![0f32; CHANNELS];
    let mut local_downsamp_iters = 0;

    // Here is the state for the monitoring part
    let mut last_monitor = Instant::now();
    let mut monitor_buf = vec![0f32; CHANNELS];
    let mut local_monitor_iters = 0;

    while let Some(payload) = receiver.recv_ref().await {
        // Compute Stokes I
        let stokes = payload.stokes_i();
        // Add to both averaging bufs
        downsamp_buf
            .iter_mut()
            .zip(&*stokes)
            .for_each(|(x, y)| *x += y);
        monitor_buf
            .iter_mut()
            .zip(&*stokes)
            .for_each(|(x, y)| *x += y);
        // Increment the counts for both
        local_downsamp_iters += 1;
        local_monitor_iters += 1;

        // Check for downsample exit condition
        if local_downsamp_iters == downsamp_iters {
            // Get a handle on the sender
            let mut send_ref = sender.send_ref().await?;
            // Write averages directly into it
            downsamp_buf
                .iter_mut()
                .for_each(|v| *v /= local_downsamp_iters as f32);
            send_ref.clone_from(&downsamp_buf);
            // And reset averaging
            downsamp_buf = vec![0f32; CHANNELS];
            local_downsamp_iters = 0;
        }

        // Check for monitor exit condition
        if last_monitor.elapsed() >= MONITOR_CADENCE {
            // Get a handle (non blocking) on the sender
            if let Ok(mut send_ref) = monitor.try_send_ref() {
                // And write averages
                monitor_buf
                    .iter_mut()
                    .for_each(|v| *v /= local_monitor_iters as f32);
                send_ref.clone_from(&monitor_buf);
            }
            // Reset averaging and timers
            last_monitor = Instant::now();
            monitor_buf = vec![0f32; CHANNELS];
            local_monitor_iters = 0;
        }
    }
    Ok(())
}
