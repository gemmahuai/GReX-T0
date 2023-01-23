//! Dumping voltage data

use crate::common::{Payload, CHANNELS};
use crossbeam_channel::{Receiver, Sender};
use hdf5::File;
use log::info;
use ndarray::{s, Array4, ArrayView, Axis};
use polling::{Event, Poller};
use std::net::UdpSocket;

/// Trigger socket event key
pub const TRIG_EVENT: usize = 42;

pub struct DumpRing {
    container: Vec<Payload>,
    write_index: usize,
}

impl DumpRing {
    #[must_use]
    pub fn new(size: usize) -> Self {
        Self {
            container: vec![Payload::default(); size],
            write_index: 0,
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    pub fn push(&mut self, payload: Payload) {
        // Tail points to the oldest data, which we will overwrite
        unsafe {
            // Safety: Mod math means this index is always correct
            *self.container.get_unchecked_mut(self.write_index) = payload;
        }
        // Then move the tail back to point to the new "oldest"
        self.write_index = (self.write_index + 1) % self.container.len();
    }

    // Pack the ring into an array of [time, (pol_a, pol_b), channel, (re, im)]
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn pack(&self) -> Array4<i8> {
        // Memory order. ndarray is row major, so the *last* index is the fastest changing
        let mut buf = Array4::zeros((self.container.len(), 2, CHANNELS, 2));
        // Start at the "oldest" and progress to the newest
        buf.axis_iter_mut(Axis(0))
            .enumerate()
            .for_each(|(i, mut slice)| {
                let pos = (self.write_index + i) % self.container.len();
                // Safety: pos is confined to 0 to len - 1 mathematically
                let (a, b) = unsafe { self.container.get_unchecked(pos) }.packed_pols();
                let a = ArrayView::from_shape((CHANNELS, 2), a).expect("Failed to make array view");
                let b = ArrayView::from_shape((CHANNELS, 2), b).expect("Failed to make array view");
                // And assign
                slice.slice_mut(s![0, .., ..]).assign(&a);
                slice.slice_mut(s![1, .., ..]).assign(&b);
            });

        buf
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

// pub fn dump_task(
//     mut ring: DumpRing,
//     payload_reciever: &Receiver<Payload>,
//     signal_reciever: &Receiver<()>,
// ) -> ! {
//     info!("Starting voltage ringbuffer fill task!");
//     loop {
//         // First check if we need to dump, as that takes priority
//         if signal_reciever.try_recv().is_ok() {
//             info!("Dumping ringbuffer");
//             // Dump
//             let file = File::create("voltages.h5").expect("Bad filename");
//             let group = file.create_group("dir").expect("Bad directory");
//             let builder = group.new_dataset_builder();
//             // Build the data to serialize
//             let packed = ring.pack();
//             // Finalize and write
//             builder
//                 .with_data(&packed)
//                 .create("voltages")
//                 .expect("Failed to build dataset");
//         } else {
//             // If we're not dumping, we're pushing data into the ringbuffer
//             if let Ok(v) = payload_reciever.try_recv() {
//                 ring.push(v);
//             }
//         }

//     }
// }

pub fn dump_task(
    mut ring: DumpRing,
    payload_reciever: &Receiver<Payload>,
    signal_reciever: &Receiver<()>,
) -> ! {
    loop {
        if payload_reciever.try_recv().is_ok() {
            // do nothing
        }
    }
}
