//! Logic for capturing raw packets from the NIC, parsing them into payloads, and sending them to other processing threads

use crate::common::{Channel, Payload};
use log::{error, info};
use socket2::{Domain, Socket, Type};
use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, Instant},
};
use thingbuf::{
    mpsc::{Receiver, Sender},
    Recycle,
};
use tokio::net::UdpSocket;

/// FPGA UDP "Word" size (8 bytes as per CASPER docs)
const WORD_SIZE: usize = 8;
/// Size of the packet count header
const TIMESTAMP_SIZE: usize = 8;
/// Total number of bytes in the spectra block of the UDP payload
const SPECTRA_SIZE: usize = 8192;
/// Total UDP payload size
pub const PAYLOAD_SIZE: usize = SPECTRA_SIZE + TIMESTAMP_SIZE;
/// Maximum number of payloads we want in the backlog
const BACKLOG_BUFFER_PAYLOADS: usize = 1024;
/// Polling interval for stats
const STATS_POLL_DURATION: Duration = Duration::from_secs(10);

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

#[derive(thiserror::Error, Debug)]
/// Errors that can be produced from captures
pub enum Error {
    #[error("We recieved a payload which wasn't the size we expected {0}")]
    SizeMismatch(usize),
    #[error("Failed to set the recv buffer size. We tried to set {expected}, but found {found}. Check sysctl net.core.rmem_max")]
    SetRecvBufferFailed { expected: usize, found: usize },
}

type Count = u64;
pub type PayloadBytes = [u8; PAYLOAD_SIZE];
type BoxedPayloadBytes = Box<PayloadBytes>;

#[must_use]
pub fn boxed_payload() -> BoxedPayloadBytes {
    Box::new([0u8; PAYLOAD_SIZE])
}

pub struct PayloadRecycle;

impl PayloadRecycle {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Recycle<BoxedPayloadBytes> for PayloadRecycle {
    fn new_element(&self) -> BoxedPayloadBytes {
        Box::new([0u8; PAYLOAD_SIZE])
    }

    fn recycle(&self, _: &mut BoxedPayloadBytes) {
        // Do nothing, we will write to every position anyway
    }
}

pub struct Capture {
    sock: UdpSocket,
    pub backlog: HashMap<Count, PayloadBytes>,
    pub drops: usize,
    pub processed: usize,
    first_payload: bool,
    next_expected_count: Count,
}

impl Capture {
    pub fn new(port: u16) -> anyhow::Result<Self> {
        // Create UDP socket
        let socket = Socket::new(Domain::IPV4, Type::DGRAM, None)?;
        // Bind our listening address
        let address = SocketAddr::from(([0, 0, 0, 0], port));
        socket.bind(&address.into())?;
        // Reuse local address without timeout
        socket.reuse_address()?;
        // Set the buffer size to 500MB (it will read as double, for some reason)
        let sock_buf_size = 256 * 1024 * 1024 * 2;
        socket.set_recv_buffer_size(sock_buf_size)?;
        // Check
        let current_buf_size = socket.recv_buffer_size()?;
        if current_buf_size != sock_buf_size * 2 {
            return Err(Error::SetRecvBufferFailed {
                expected: sock_buf_size * 2,
                found: current_buf_size,
            }
            .into());
        }
        // Replace the socket2 socket with a tokio socket
        let sock = UdpSocket::from_std(socket.into())?;
        Ok(Self {
            sock,
            backlog: HashMap::with_capacity(BACKLOG_BUFFER_PAYLOADS),
            drops: 0,
            processed: 0,
            first_payload: true,
            next_expected_count: 0,
        })
    }

    pub async fn capture(&mut self, buf: &mut PayloadBytes) -> anyhow::Result<()> {
        let n = self.sock.recv(buf).await?;
        if n != buf.len() {
            Err(Error::SizeMismatch(n).into())
        } else {
            Ok(())
        }
    }

    pub async fn start(
        &mut self,
        payload_sender: Sender<BoxedPayloadBytes, PayloadRecycle>,
        stats_send: Sender<Stats>,
        stats_polling_time: Duration,
    ) -> anyhow::Result<()> {
        let mut last_stats = Instant::now();
        loop {
            // Send away the stats if the time has come (non blocking)
            if last_stats.elapsed() >= stats_polling_time {
                if let Ok(mut send) = stats_send.try_send_ref() {
                    *send = Stats {
                        drops: self.drops,
                        processed: self.processed,
                    };
                }
                last_stats = Instant::now();
            }
            // Grab the next slot
            let mut slot = payload_sender.send_ref().await?;
            // By default, capture into the slot
            self.capture(&mut *slot).await?;
            self.processed += 1;
            // Then, we get the count
            let this_count = count(&*slot);
            // Check first payload
            if self.first_payload {
                self.first_payload = false;
                self.next_expected_count = this_count + 1;
                continue;
            } else if this_count == self.next_expected_count {
                self.next_expected_count += 1;
                continue;
            } else if this_count < self.next_expected_count {
                // If the packet is from the past, we drop it
                self.drops += 1;
            } else {
                // This packet is from the future, store it
                self.backlog.insert(this_count, **slot);
            }
            // If we got this far, this means we need to either replace the value of this slot with one from the backlog, or zeros
            if let Some(payload) = self.backlog.remove(&self.next_expected_count) {
                (**slot).clone_from(&payload);
            } else {
                // Nothing we can do, write zeros
                (**slot).clone_from(&[0u8; PAYLOAD_SIZE]);
                (**slot)[0..8].clone_from_slice(&this_count.to_be_bytes());
                self.drops += 1;
            }
        }
    }
}

/// Decode just the count from a byte array
fn count(pl: &PayloadBytes) -> Count {
    u64::from_be_bytes(pl[0..8].try_into().unwrap())
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub drops: usize,
    pub processed: usize,
}

pub async fn cap_task(
    port: u16,
    cap_send: Sender<BoxedPayloadBytes, PayloadRecycle>,
    stats_send: Sender<Stats>,
) -> anyhow::Result<()> {
    info!("Starting capture task!");
    let mut cap = Capture::new(port).unwrap();
    cap.start(cap_send, stats_send, STATS_POLL_DURATION).await
}

// This task will decode incoming packets and send to the ringbuffer and downsample tasks
pub async fn decode_split_task(
    from_cap: Receiver<BoxedPayloadBytes, PayloadRecycle>,
    to_downsample: Sender<Payload>,
    to_dumps: Sender<Payload>,
) -> anyhow::Result<()> {
    info!("Starting decode-split");
    // Receive
    while let Some(payload) = from_cap.recv_ref().await {
        // Grab block
        let mut send_ref = to_downsample.send_ref().await?;
        // Decode directly into block
        *send_ref = Payload::from_bytes(&**payload);
        // This one won't cause backpressure because that only will happen when we're doing IO
        let _result = to_dumps.try_send(*send_ref);
        // Lexical drop sends it away
    }
    Ok(())
}
