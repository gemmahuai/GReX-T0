//! Logic for capturing raw packets from the NIC, parsing them into payloads, and sending them to other processing threads

use crate::common::Payload;
use crossbeam_channel::Sender;
use num_complex::Complex;
use pcap::Stat;

/// FPGA UDP "Word" size (8 bytes as per CASPER docs)
const WORD_SIZE: usize = 8;
/// Size of the packet count header
const TIMESTAMP_SIZE: usize = 8;
// UDP Header size (spec-defined)
const UDP_HEADER_SIZE: usize = 42;
/// Total number of bytes in the spectra block of the UDP payload
const SPECTRA_SIZE: usize = 8192;
/// Total UDP payload size
const PAYLOAD_SIZE: usize = SPECTRA_SIZE + TIMESTAMP_SIZE;
/// How many packets before we send statistics information to another thread
/// This should be around ~4s
const STAT_PACKET_INTERVAL: usize = 500_000;

impl Payload {
    /// Construct a payload instance from a raw UDP payload
    #[allow(clippy::cast_possible_wrap)]
    fn from_bytes(bytes: &[u8]) -> Self {
        let mut payload = Payload::default();
        for (i, word) in bytes[TIMESTAMP_SIZE..].chunks_exact(WORD_SIZE).enumerate() {
            // Each word contains two frequencies for each polarization
            // [A1 B1 A2 B2]
            // Where each channel is [Re Im] as FixedI8<7>
            payload.pol_a[2 * i] = Complex::new(word[0] as i8, word[1] as i8);
            payload.pol_a[2 * i + 1] = Complex::new(word[4] as i8, word[5] as i8);
            payload.pol_b[2 * i] = Complex::new(word[2] as i8, word[3] as i8);
            payload.pol_b[2 * i + 1] = Complex::new(word[6] as i8, word[7] as i8);
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
            .open()
            .expect("Failed to open the capture");
        // Add the port filter
        cap.filter(&format!("dst port {port}"), true)
            .expect("Error creating port filter");
        // And return
        Capture(cap)
    }

    fn next_payload(&mut self) -> Option<Payload> {
        let pak = self.0.next_packet().ok()?;
        if pak.data.len() != (PAYLOAD_SIZE + UDP_HEADER_SIZE) {
            return None;
        }
        Some(Payload::from_bytes(&pak.data[UDP_HEADER_SIZE..]))
    }
}

#[allow(clippy::missing_panics_doc)]
pub fn pcap_task(
    mut cap: Capture,
    payload_sender: &Sender<Payload>,
    stat_sender: &Sender<Stat>,
) -> ! {
    println!("Starting capture task!");
    let mut count = 0;
    loop {
        if count == STAT_PACKET_INTERVAL {
            count = 0;
            stat_sender
                .send(cap.0.stats().expect("Getting capture statistics failed"))
                .expect("Sending capture statistics failed");
        }
        if let Some(payload) = cap.next_payload() {
            if payload_sender.try_send(payload).is_ok() {
                count += 1;
            }
        }
    }
}
