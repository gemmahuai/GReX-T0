//! Logic for capturing raw packets from the NIC, parsing them into payloads, and sending them to other processing threads

use crate::common::{Channel, Payload};
use crossbeam::channel::Sender;
use log::{debug, info, warn};
use pcap::Stat;
use std::time::{Duration, Instant};

/// FPGA UDP "Word" size (8 bytes as per CASPER docs)
const WORD_SIZE: usize = 8;
/// Size of the packet count header
const TIMESTAMP_SIZE: usize = 8;
// UDP Header size (spec-defined)
const UDP_HEADER_SIZE: usize = 42;
/// Total number of bytes in the spectra block of the UDP payload
const SPECTRA_SIZE: usize = 8192;
/// Total UDP payload size
pub const PAYLOAD_SIZE: usize = SPECTRA_SIZE + TIMESTAMP_SIZE;
/// How many packets before we send statistics information to another thread
/// This should be around ~4s
const STAT_PACKET_INTERVAL: usize = 500_000;

impl Payload {
    /// Construct a payload instance from a raw UDP payload
    #[allow(clippy::cast_possible_wrap)]
    #[must_use]
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut payload = Payload::default();
        for (i, word) in bytes[TIMESTAMP_SIZE..].chunks_exact(WORD_SIZE).enumerate() {
            // Each word contains two frequencies for each polarization
            // [A1 B1 A2 B2]
            // Where each channel is [Re Im] as FixedI8<7>
            payload.pol_a[2 * i] = Channel::new(word[0] as i8, word[1] as i8);
            payload.pol_a[2 * i + 1] = Channel::new(word[4] as i8, word[5] as i8);
            payload.pol_b[2 * i] = Channel::new(word[2] as i8, word[3] as i8);
            payload.pol_b[2 * i + 1] = Channel::new(word[6] as i8, word[7] as i8);
        }
        // Then unpack the timestamp/order
        payload.count = u64::from_be_bytes(
            bytes[0..TIMESTAMP_SIZE]
                .try_into()
                .expect("This is exactly 8 bytes"),
        );
        payload
    }
}

pub struct Capture(pcap::Capture<pcap::Active>);

impl Capture {
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn new(device_name: &str, port: u16) -> Self {
        // Grab the pcap device that matches this interface
        let device = pcap::Device::list()
            .expect("Error listing devices from Pcap")
            .into_iter()
            .find(|d| d.name == device_name)
            .unwrap_or_else(|| panic!("Device named {device_name} not found"));
        // Create the "capture"
        let mut cap = pcap::Capture::from_device(device)
            .expect("Failed to create capture")
            .buffer_size(33_554_432) // Up to 20ms
            .open()
            .expect("Failed to open the capture");
        //.setnonblock()
        //.expect("Setting non-blocking mode failed");
        // Add the port filter
        cap.filter(&format!("dst port {port}"), true)
            .expect("Error creating port filter");
        // And return
        Capture(cap)
    }

    fn next_payload(&mut self) -> Option<RawPacket> {
        let p = self.0.next_packet().unwrap();
        if p.data.len() == (PAYLOAD_SIZE + UDP_HEADER_SIZE) {
            Some(
                p.data[UDP_HEADER_SIZE..]
                    .try_into()
                    .expect("We've already checked the size"),
            )
        } else {
            None
        }
    }
}

pub type RawPacket = [u8; PAYLOAD_SIZE];

#[allow(clippy::missing_panics_doc)]
pub fn pcap_task(
    mut cap: Capture,
    payload_sender: &Sender<Payload>,
    stat_sender: &Sender<(Stat, Duration)>,
) -> ! {
    info!("Starting capture task!");
    let mut count = 0;
    let mut last_time = Instant::now();
    loop {
        if count == STAT_PACKET_INTERVAL {
            count = 0;
            // We don't care about dropping stats, we *do* care about dropping packets
            let _ = stat_sender.try_send((
                cap.0.stats().expect("Failed to get capture statistics"),
                last_time.elapsed(),
            ));
            last_time = Instant::now();
        }
        if let Some(bytes) = cap.next_payload() {
            let pl = Payload::from_bytes(&bytes);
            info!("Incoming packet idx: {}", pl.count);
            payload_sender.send(pl).unwrap();
            count += 1;
        }
    }
}
