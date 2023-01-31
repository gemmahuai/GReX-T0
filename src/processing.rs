//! Inter-thread processing (downsampling, voltage ring buffer, etc)

use std::collections::HashMap;

use crate::common::{Payload, Stokes, CHANNELS};
use crossbeam::channel::{Receiver, Sender};
use log::{info, warn};

// About 10s
const MONITOR_SPEC_DOWNSAMPLE_FACTOR: usize = 305_180;
// How many packets out of order do we want to be able to deal with? Vikram says 1000 is fine.
const PACKET_REODER_BUF_SIZE: usize = 16_384;

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

struct ReorderBuf {
    buf: Vec<Payload>,
    free_idxs: Vec<usize>,
    queued: HashMap<u64, usize>,
    next_needed: u64,
}

impl ReorderBuf {
    fn new() -> Self {
        Self {
            buf: vec![Payload::default(); PACKET_REODER_BUF_SIZE],
            free_idxs: (0..PACKET_REODER_BUF_SIZE).collect(),
            queued: HashMap::new(),
            next_needed: 0,
        }
    }

    /// Attempts to add a payload to the buffer, returning None if the buffer is full
    fn push(&mut self, payload: &Payload) -> Option<()> {
        // Get the next free index
        let next_idx = self.free_idxs.pop()?;
        // Associate its timestamp
        self.queued.insert(payload.count, next_idx);
        // Insert into the buffer
        self.buf[next_idx] = *payload;
        Some(())
    }

    /// Set the next payload needed for the iterator to work
    fn set_needed(&mut self, next_needed: u64) {
        self.next_needed = next_needed;
    }

    /// Get the state of the next needed (after iteration has moved it)
    fn get_needed(&self) -> u64 {
        self.next_needed
    }

    /// Clear the state of the whole thing (without actually overwriting memory)
    fn reset(&mut self) {
        self.queued.clear();
        self.free_idxs = (0..PACKET_REODER_BUF_SIZE).collect();
    }
}

impl Iterator for ReorderBuf {
    type Item = Payload;

    fn next(&mut self) -> Option<Self::Item> {
        // Grab the index of the next needed (if it exists)
        let idx = self.queued.remove(&self.next_needed)?;
        // Recycle this index (mark it as free)
        self.free_idxs.push(idx);
        // Increment next needed
        self.next_needed += 1;
        // Return the payload
        Some(self.buf[idx])
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_reorder() {
        let mut rb = ReorderBuf::new();
        for i in (0..=127).rev() {
            rb.push(&Payload {
                count: i,
                ..Default::default()
            });
        }
        for (i, payload) in rb.by_ref().enumerate() {
            assert_eq!(payload.count, i as u64);
        }
    }
}

#[allow(clippy::missing_panics_doc)]
pub fn reorder_task(payload_recv: &Receiver<Payload>, payload_send: &Sender<Payload>) -> ! {
    let mut rb = ReorderBuf::new();
    let mut first_payload = true;
    loop {
        // Grab the next payload
        let payload = payload_recv.recv().unwrap();
        // If this is the next payload we expect (or it's the first one), send it right away
        // which avoids a copy in the not-out-of-order case
        if first_payload || payload.count == rb.get_needed() {
            rb.set_needed(payload.count + 1);
            payload_send.send(payload).unwrap();
            if first_payload {
                first_payload = false;
            }
        } else {
            // Insert in our buffer
            // If we run out of free slots (maybe the packet is totally gone (got corrupted or something)) we need to:
            // - Reset the whole thing
            // - Set the next needed to *this* payload's count + 1
            // - Send off this packet that we couldn't push
            if rb.push(&payload).is_none() {
                let oldest = rb.queued.keys().min().unwrap();
                let newest = rb.queued.keys().max().unwrap();
                warn!(
                    "Reorder buffer filled up while waiting for next payload. Oldest {} - Newest {}",
                    oldest, newest
                );
                rb.reset();
                rb.set_needed(payload.count + 1);
                payload_send.send(payload).unwrap();
            }
        }
        // Drain
        for pl in rb.by_ref() {
            payload_send.send(pl).unwrap();
        }
    }
}
