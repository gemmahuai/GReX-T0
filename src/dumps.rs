//! Dumping voltage data

use crate::common::{Payload, CHANNELS};
use chrono::Utc;
use crossbeam::{
    channel::{Receiver, Sender},
    queue::ArrayQueue,
};
use hdf5::File;
use log::{info, warn};
use ndarray::{s, Array4, ArrayView, Axis};
use polling::{Event, Poller};
use std::net::UdpSocket;

/// Trigger socket event key
pub const TRIG_EVENT: usize = 42;

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
    pub fn dump(&self) -> anyhow::Result<()> {
        // Filename with ISO 8610 standard format
        let filename = format!("grex_dump-{}.h5", Utc::now().format("%Y%m%dT%H%M%S"));
        let file = File::create(filename)?;
        let ds = file
            .new_dataset::<i8>()
            .chunk((1, 2, CHANNELS, 2))
            .shape((self.container.len(), 2, CHANNELS, 2))
            .deflate(3)
            .create("voltages")?;
        // And then write in chunks, draining the buffer
        let mut idx = 0;
        while let Some(pl) = self.container.pop() {
            ds.write_slice(&pl.into_ndarray(), (idx, .., .., ..))?;
            idx += 1;
        }
        Ok(())
    }
}

#[allow(clippy::missing_panics_doc)]
pub fn trigger_task(signal_sender: &Sender<()>, socket: &UdpSocket) -> ! {
    info!("Starting voltage ringbuffer trigger task!");
    // Maybe even 0 would work, we don't expect data
    let mut buf = [0; 10];
    // Create a poller and register interest in readability on the socket.
    let poller = Poller::new().unwrap();
    poller.add(socket, Event::readable(TRIG_EVENT)).unwrap();
    // Socket event loop.
    let mut events = Vec::new();
    loop {
        // Wait for at least one I/O event.
        events.clear();
        poller.wait(&mut events, None).unwrap();
        for ev in &events {
            if ev.key == TRIG_EVENT {
                match socket.recv_from(&mut buf) {
                    Ok(_) => signal_sender.send(()).unwrap(),
                    Err(e) => panic!("encountered IO error: {e}"),
                }
                poller.modify(socket, Event::readable(TRIG_EVENT)).unwrap();
            }
        }
    }
}

#[allow(clippy::missing_panics_doc)]
pub fn dump_task(
    mut ring: DumpRing,
    payload_reciever: &Receiver<Payload>,
    signal_reciever: &Receiver<()>,
) -> ! {
    info!("Starting voltage ringbuffer fill task!");
    loop {
        // First check if we need to dump, as that takes priority
        if signal_reciever.try_recv().is_ok() {
            info!("Dumping ringbuffer");
            match ring.dump() {
                Ok(_) => (),
                Err(e) => warn!("Error in dumping buffer - {}", e),
            }
        } else {
            // If we're not dumping, we're pushing data into the ringbuffer
            let payload = payload_reciever.recv().unwrap();
            ring.push(payload);
        }
    }
}
