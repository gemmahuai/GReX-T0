//! Logic for capturing raw packets from the NIC, parsing them into payloads, and sending them to other processing threads

use crate::common::{Channel, Payload, Payloads};
use anyhow::bail;
use crossbeam::channel::{Receiver, Sender};
use lazy_static::lazy_static;
use log::{error, info, warn};
use nix::{
    errno::{errno, Errno},
    libc::{iovec, mmsghdr, msghdr, recvfrom, recvmmsg, MSG_DONTWAIT},
};
use socket2::{Domain, Socket, Type};
use std::{
    collections::{HashMap, HashSet},
    ffi::c_void,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    os::unix::prelude::AsRawFd,
    ptr::null_mut,
    sync::{Arc, Mutex},
};

/// FPGA UDP "Word" size (8 bytes as per CASPER docs)
const WORD_SIZE: usize = 8;
/// Size of the packet count header
const TIMESTAMP_SIZE: usize = 8;
/// Total number of bytes in the spectra block of the UDP payload
const SPECTRA_SIZE: usize = 8192;
/// Total UDP payload size
pub const PAYLOAD_SIZE: usize = SPECTRA_SIZE + TIMESTAMP_SIZE;
// Linux setting
const RMEM_MAX: usize = 2_097_152;
const PACKETS_PER_CAPTURE: usize = 32768;
// Try to clear the FIFOs
const WARMUP_CHUNKS: usize = 1;

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

pub struct Capture {
    sock: Socket,
    msgs: Vec<mmsghdr>,
    buffers: Vec<Vec<u8>>,
    _iovecs: Vec<iovec>,
}

impl Capture {
    /// Create and setup a new "bulk" capture using recvmmsg
    #[allow(clippy::missing_errors_doc)]
    pub fn new(port: u16, packets_per_capture: usize, packet_size: usize) -> anyhow::Result<Self> {
        // Create the bog-standard UDP socket
        let sock = Socket::new(Domain::IPV4, Type::DGRAM, None)?;
        // Create its local address and bind
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port);
        sock.bind(&addr.into())?;
        // Make the recieve buffer huge
        sock.set_recv_buffer_size(RMEM_MAX)?;
        // Create the arrays on the heap to point the NIC to
        let mut buffers = vec![vec![0u8; packet_size]; packets_per_capture];
        // And connect up the scatter-gather buffers
        // Crazy this isn't unsafe
        let mut iovecs: Vec<_> = buffers
            .iter_mut()
            .map(|ptr| iovec {
                iov_base: ptr.as_mut_ptr().cast::<c_void>(),
                iov_len: packet_size,
            })
            .collect();
        let msgs: Vec<_> = iovecs
            .iter_mut()
            .map(|ptr| mmsghdr {
                msg_hdr: msghdr {
                    msg_name: null_mut(),
                    msg_namelen: 0,
                    msg_iov: ptr as *mut iovec,
                    msg_iovlen: 1,
                    msg_control: null_mut(),
                    msg_controllen: 0,
                    msg_flags: 0,
                },
                msg_len: 0,
            })
            .collect();
        Ok(Self {
            sock,
            msgs,
            buffers,
            _iovecs: iovecs,
        })
    }

    // Wait for the capture to complete and return a reference to the vector of byte vectors
    // Once we have rust 1.66 in guix, this should be a lending iterator-type thing
    #[allow(clippy::missing_errors_doc)]
    #[allow(clippy::missing_panics_doc)]
    #[allow(clippy::cast_possible_wrap)]
    #[allow(clippy::cast_possible_truncation)]
    pub fn capture(&mut self) -> anyhow::Result<&[Vec<u8>]> {
        let ret = unsafe {
            recvmmsg(
                self.sock.as_raw_fd(),
                self.msgs.as_mut_ptr(),
                self.buffers.len().try_into().unwrap(),
                0,
                null_mut(),
            )
        };
        if ret != self.buffers.len() as i32 {
            bail!("Not enough packets");
        }
        if ret == -1 {
            bail!("Capture Error {:#?}", Errno::from_i32(errno()));
        }
        Ok(&self.buffers)
    }

    /// Clear the recieve buffers
    #[allow(clippy::missing_errors_doc)]
    pub fn clear(&mut self) -> anyhow::Result<()> {
        let size = self.buffers[0].len();
        let mut buf = vec![0u8; size];
        loop {
            let ret = unsafe {
                recvfrom(
                    self.sock.as_raw_fd(),
                    buf.as_mut_ptr().cast::<c_void>(),
                    size,
                    MSG_DONTWAIT,
                    null_mut(),
                    null_mut(),
                )
            };
            if ret == -1 {
                match Errno::from_i32(errno()) {
                    Errno::EAGAIN => return Ok(()),
                    _ => bail!("Socket error: {:#?}", Errno::from_i32(errno())),
                }
            }
        }
    }
}

struct PayloadBuf {
    map: HashMap<u64, usize>,
    buf: Vec<Payload>,
    free_idxs: Vec<usize>,
}

impl PayloadBuf {
    fn new(size: usize) -> Self {
        Self {
            map: HashMap::new(),
            buf: vec![Payload::default(); size],
            free_idxs: (0..size).collect(),
        }
    }
    fn remove(&mut self, count: &u64) -> Option<Payload> {
        let idx = self.map.remove(count)?;
        self.free_idxs.push(idx);
        Some(self.buf[idx])
    }
    fn insert(&mut self, payload: &Payload) -> Option<()> {
        let next_idx = self.free_idxs.pop()?;
        self.map.insert(payload.count, next_idx);
        self.buf[next_idx] = *payload;
        Some(())
    }
    fn clear(&mut self) {
        self.map.clear();
        self.free_idxs = (0..self.buf.len()).collect();
    }
    fn len(&self) -> usize {
        self.map.len()
    }
}

lazy_static! {
    static ref UNSORTED_PAYLOADS: Arc<Mutex<PayloadBuf>> =
        Arc::new(Mutex::new(PayloadBuf::new(32768)));
}

/// Returns sorted payloads, how many we dropped in the process
#[allow(clippy::cast_possible_truncation)]
fn stateful_sort(payloads: Payloads, oldest_count: u64, target: &mut Payloads) -> usize {
    let mut drops = 0usize;
    let mut to_fill =
        (oldest_count..(oldest_count + PACKETS_PER_CAPTURE as u64)).collect::<HashSet<_>>();
    // Get a local ref to the global buffer
    let mut unsorted = UNSORTED_PAYLOADS.lock().unwrap();

    // For each payload in the input, find the slot it corresponds to in the output and insert it
    for payload in payloads {
        if payload.count < oldest_count {
            // If it is from the past, throw it out and increment the drop count
            drops += 1;
        } else if payload.count >= (oldest_count + PACKETS_PER_CAPTURE as u64) {
            // If it is for the future, add to the hashmap
            if unsorted.insert(&payload).is_none() {
                error!("Reorder overflow");
                unsorted.clear();
            }
        } else {
            // Mark that this spot is filled
            to_fill.remove(&payload.count);
            // And insert it into sorted
            target[(payload.count - oldest_count) as usize] = payload;
        }
    }

    if to_fill.len() == PACKETS_PER_CAPTURE {
        warn!("Not a single payload in this chunk was expected, moving expected count forward");
        let dropped = PACKETS_PER_CAPTURE + unsorted.len();
        unsorted.clear();
        return dropped;
    }

    // Now fill in the gaps with data we have
    for missing_count in to_fill {
        if let Some(pl) = unsorted.remove(&missing_count) {
            target[(pl.count - oldest_count) as usize] = pl;
        }
    }

    // And everything else are zeros
    drops
}

pub struct Stats {
    pub captured: u64,
    pub dropped: u64,
}

#[allow(clippy::missing_panics_doc)]
pub fn cap_task(port: u16, cap_send: &Sender<Vec<Vec<u8>>>) {
    info!("Starting capture task!");
    // We have to construct the capture on this thread because the CFFI stuff isn't thread-safe
    let mut cap = Capture::new(port, PACKETS_PER_CAPTURE, PAYLOAD_SIZE).unwrap();
    cap.clear().unwrap();
    info!("Warming up capture thread");
    // Clear out FIFOs
    for _ in 0..WARMUP_CHUNKS {
        let _ = cap.capture().unwrap();
    }
    info!("Starting pipeline");
    loop {
        // Capture a chunk of payloads
        let chunk = cap.capture().unwrap();
        // Send
        cap_send.send(chunk.to_vec()).unwrap();
    }
}

#[allow(clippy::missing_panics_doc)]
// This task will decode incoming packets, sort by index, and send to the ringbuffer and downsample tasks
pub fn sort_split_task(
    from_cap: &Receiver<Vec<Vec<u8>>>,
    to_downsample: &Sender<Payloads>,
    to_dumps: &Sender<Payloads>,
    to_monitor: &Sender<Stats>,
) -> ! {
    let mut oldest_count = None;
    loop {
        // Receive
        let packets = from_cap.recv().unwrap();
        // Decode
        let payloads: Vec<_> = packets
            .iter()
            .map(|bytes| Payload::from_bytes(bytes))
            .collect();
        // First iter edge case
        if oldest_count.is_none() {
            oldest_count = Some(payloads.iter().map(|p| p.count).min().unwrap());
        }

        let counts: Vec<_> = payloads.iter().map(|p| p.count).collect();

        println!(
            "Payload Chunk Counts {}-{}, Target Chunk Range {}-{}",
            counts.iter().min().unwrap(),
            counts.iter().max().unwrap(),
            oldest_count.unwrap(),
            oldest_count.unwrap() + PACKETS_PER_CAPTURE as u64 - 1
        );

        // Sort
        let sorted = payloads.clone();
        //let dropped = stateful_sort(payloads, oldest_count.unwrap(), &mut sorted);
        // Send
        to_downsample.send(sorted.clone()).unwrap();
        // This one won't cause backpressure because that only will happen when we're doing IO
        let _result = to_dumps.try_send(sorted);
        // And then increment our next expected oldest
        oldest_count = Some(oldest_count.unwrap() + PACKETS_PER_CAPTURE as u64);

        // Send stats (no backpressure)
        let _ = to_monitor.try_send(Stats {
            captured: PACKETS_PER_CAPTURE as u64,
            dropped: 0usize as u64,
        });
    }
}
