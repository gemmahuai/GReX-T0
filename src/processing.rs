//! Inter-thread processing (downsampling, etc)

use crate::common::{Payload, Stokes, CHANNELS};
use log::info;
use thingbuf::mpsc::{Receiver, Sender};

#[allow(clippy::missing_panics_doc)]
pub async fn downsample_task(
    receiver: Receiver<Payload>,
    sender: Sender<Stokes>,
    downsample_power: u32,
) -> anyhow::Result<()> {
    info!("Starting downsample");
    let mut avg_buf = vec![0f32; CHANNELS];
    let mut avg_iters = 0;
    let iters = 2usize.pow(downsample_power);
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
            // And reset averaging
            avg_buf = vec![0f32; CHANNELS];
            avg_iters = 0;
        }
    }
    Ok(())
}
