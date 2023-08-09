//! Inter-thread processing (downsampling, etc)
use crate::common::{Payload, Stokes, CHANNELS};
use eyre::{bail, eyre};
use log::info;
use std::time::{Duration, Instant};
use thingbuf::mpsc::blocking::{Sender, StaticReceiver, StaticSender};

/// How long before we send one off to monitor
const MONITOR_CADENCE: Duration = Duration::from_secs(10);

#[allow(clippy::missing_panics_doc)]
pub fn downsample_task(
    receiver: StaticReceiver<Payload>,
    sender: Sender<Stokes>,
    to_dumps: StaticSender<Payload>,
    monitor: Sender<Stokes>,
    downsample_power: u32,
) -> eyre::Result<()> {
    info!("Starting downsample");

    // We have two averaging states, one for the normal downsample process and one for monitoring
    // They differ in that the standard "thru" connection is averaging by counts and the monitoing one is averaging by time
    let downsamp_iters = 2usize.pow(downsample_power);
    let mut downsamp_buf = [0f32; CHANNELS];
    let mut local_downsamp_iters = 0;

    // Here is the state for the monitoring part
    let mut last_monitor = Instant::now();
    let mut monitor_buf = [0f32; CHANNELS];
    let mut local_monitor_iters = 0;

    loop {
        let payload = receiver.recv_ref().ok_or_else(|| eyre!("Channel closed"))?;
        // Compute Stokes I
        let stokes = payload.stokes_i();
        // Send payload to dump (non-blocking)
        if let Err(thingbuf::mpsc::errors::TrySendError::Closed(_)) = to_dumps.try_send(*payload) {
            bail!("Channel closed")
        }
        debug_assert_eq!(stokes.len(), CHANNELS);
        // Add to averaging bufs
        downsamp_buf
            .iter_mut()
            .zip(&stokes)
            .for_each(|(x, y)| *x += y);

        // Increment the count
        local_downsamp_iters += 1;

        // Check for downsample exit condition
        if local_downsamp_iters == downsamp_iters {
            // Write averages directly into it
            downsamp_buf
                .iter_mut()
                .for_each(|v| *v /= local_downsamp_iters as f32);
            sender.send(downsamp_buf.try_into().unwrap())?;

            // Then, use *this* average to save us some cycles for the monitoring
            monitor_buf
                .iter_mut()
                .zip(&downsamp_buf)
                .for_each(|(x, y)| *x += y);
            local_monitor_iters += 1;

            // And reset averaging
            downsamp_buf.iter_mut().for_each(|v| *v = 0.0);
            local_downsamp_iters = 0;

            //Check for monitor exit condition
            if last_monitor.elapsed() >= MONITOR_CADENCE {
                // And write averages
                monitor_buf
                    .iter_mut()
                    .for_each(|v| *v /= local_monitor_iters as f32);
                // Get a handle (non blocking) on the sender
                monitor.send(monitor_buf.try_into().unwrap())?;
                // Reset averaging and timers
                last_monitor = Instant::now();
                monitor_buf.iter_mut().for_each(|v| *v = 0.0);
                local_monitor_iters = 0;
            }
        }
    }
}
