//! Dumping voltage data

use crate::common::{Payload, CHANNELS};
use chrono::{DateTime, Utc};
use crossbeam_queue::ArrayQueue;
use hdf5::File;
use log::{info, warn};
use std::net::SocketAddr;
use thingbuf::mpsc::{Receiver, Sender};
use tokio::net::UdpSocket;

pub struct DumpRing {
    container: ArrayQueue<Payload>,
}

impl DumpRing {
    #[must_use]
    pub fn new(size: usize) -> Self {
        Self {
            container: ArrayQueue::new(size),
        }
    }

    pub fn push(&mut self, payload: Payload) {
        self.container.force_push(payload);
    }

    // Pack the ring into an array of [time, (pol_a, pol_b), channel, (re, im)]
    #[allow(clippy::missing_errors_doc)]
    pub fn dump(&self, start_time: &DateTime<Utc>) -> anyhow::Result<()> {
        // Filename with ISO 8610 standard format
        let filename = format!("grex_dump-{}.h5", Utc::now().format("%Y%m%dT%H%M%S"));
        let file = File::create(filename)?;
        let ds = file
            .new_dataset::<i8>()
            .chunk((1, 2, CHANNELS, 2))
            .shape((self.container.len(), 2, CHANNELS, 2))
            .create("voltages")?;
        // And then write in chunks, draining the buffer
        let mut idx = 0;
        let mut payload_time = *start_time;
        while let Some(pl) = self.container.pop() {
            ds.write_slice(&pl.into_ndarray(), (idx, .., .., ..))?;
            payload_time = pl.real_time(start_time);
            idx += 1;
        }
        // Set the time attribute
        let attr = ds.new_attr::<i64>().create("timestamp")?;
        attr.write_scalar(&payload_time.timestamp_micros())?;
        Ok(())
    }
}

#[allow(clippy::missing_panics_doc)]
pub async fn trigger_task(sender: Sender<()>, port: u16) -> anyhow::Result<()> {
    info!("Starting voltage ringbuffer trigger task!");
    // Create the socket
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let sock = UdpSocket::bind(addr).await?;
    // Maybe even 0 would work, we don't expect data
    let mut buf = [0; 10];
    loop {
        sock.recv_from(&mut buf).await?;
        sender.send(()).await?;
    }
}

#[allow(clippy::missing_panics_doc)]
pub async fn dump_task(
    payload_reciever: Receiver<Payload>,
    signal_reciever: Receiver<()>,
    start_time: DateTime<Utc>,
    vbuf_power: u32,
) -> anyhow::Result<()> {
    info!("Starting voltage ringbuffer fill task!");
    // Create the ring buffer to store voltage dumps
    let mut ring = DumpRing::new(2usize.pow(vbuf_power));
    loop {
        // First check if we need to dump, as that takes priority
        if signal_reciever.try_recv().is_ok() {
            info!("Dumping ringbuffer");
            match ring.dump(&start_time) {
                Ok(_) => (),
                Err(e) => warn!("Error in dumping buffer - {}", e),
            }
        } else {
            // If we're not dumping, we're pushing data into the ringbuffer
            if let Some(payload) = payload_reciever.recv().await {
                ring.push(payload);
            } else {
                break;
            }
        }
    }
    Ok(())
}
