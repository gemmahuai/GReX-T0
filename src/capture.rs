use nix::{
    libc::{setsockopt, sockaddr_ll, SOL_PACKET},
    sys::{
        mman::{mmap, munmap, MapFlags, ProtFlags},
        socket::{socket, AddressFamily, SockFlag, SockProtocol, SockType},
    },
    unistd::{close, sysconf, SysconfVar},
};
use std::{
    ffi::{c_int, c_uint, c_ulong, c_ushort, c_void},
    mem::size_of,
    num::NonZeroUsize,
    os::fd::RawFd,
};

/// The number of frames in the ring
const CONF_RING_FRAMES: usize = 128;
const PACKET_RX_RING: c_int = 5;
const ETH_HLEN: usize = 14;
const TPACKET_ALIGNMENT: usize = 16;
const fn tpacket_align(x: usize) -> usize {
    ((x) + TPACKET_ALIGNMENT - 1) & !(TPACKET_ALIGNMENT - 1)
}
const TPACKET_HDRLEN: usize = tpacket_align(size_of::<TPacketHdr>()) + size_of::<sockaddr_ll>();

#[derive(Debug)]
pub struct UdpCap {
    fd: RawFd,
    req: TPacketReq,
    frame_idx: usize,
    frame_ptr: *mut u8,
    rx_ring: *mut u8,
}

#[repr(C)]
#[derive(Debug, Default)]
struct TPacketReq {
    /// Minimal size of contiguous block
    block_size: c_uint,
    /// Number of blocks
    block_number: c_uint,
    /// Size of frame
    frame_size: c_uint,
    /// Total number of frames
    frame_number: c_uint,
}

#[repr(C)]
#[derive(Debug, Default)]
struct TPacketHdr {
    status: c_ulong,
    len: c_uint,
    snaplen: c_uint,
    mac: c_ushort,
    net: c_ushort,
    sec: c_uint,
    usec: c_uint,
}

impl UdpCap {
    pub fn new(mtu: usize) -> Self {
        // Create the AF_PACKET socket file descriptor
        // We want to use TPACKET_V2 to maximize perf on 64-bit systems.
        // We don't need TPACKET_V3 as technically we know we are recieving
        // fixed size packets.
        let fd = socket(
            AddressFamily::Packet,
            SockType::Datagram,
            SockFlag::empty(),
            SockProtocol::Udp,
        )
        .expect("Creating the raw socket failed");
        // Start to set up the ring buffer request
        let mut req = TPacketReq::default();
        // Frames will be header + MTU
        req.frame_size = (tpacket_align(TPACKET_HDRLEN + ETH_HLEN) + tpacket_align(mtu)) as c_uint;
        // Blocks are the smallest power of two multiple of the page size that is at least the frame size
        let page_size = sysconf(SysconfVar::PAGE_SIZE)
            .expect("Unable to get the page size")
            .unwrap() as u32;
        req.block_size = (req.frame_size / page_size + 1).next_power_of_two();
        req.block_number = CONF_RING_FRAMES as u32;
        req.frame_number = req.block_number * req.block_size / req.frame_size;
        // And finally perform the request
        // Safety: FD is valid because we've used a safe API, req exists, and we're checking the return
        if unsafe {
            setsockopt(
                fd,
                SOL_PACKET,
                PACKET_RX_RING,
                &req as *const _ as *const c_void,
                size_of::<TPacketReq>() as u32,
            )
        } == -1
        {
            panic!(
                "Setting the PACKET_RX_RING option failed: {:#?}",
                std::io::Error::last_os_error()
            );
        }
        // Now memmory-map the ring into this process
        let rx_ring = unsafe {
            mmap(
                None,
                NonZeroUsize::new((req.block_number * req.block_size) as usize).unwrap(),
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED,
                fd,
                0,
            )
            .expect("Creating the mmap-ed ring buffer failed") as *mut u8
        };

        Self {
            fd,
            req,
            frame_idx: 0,
            frame_ptr: rx_ring,
            rx_ring,
        }
    }

    fn recv_next_block(&mut self) {
        // Handle frame
        let frames_per_buffer = (self.req.block_size / self.req.block_number) as usize;
        // Increment frame index, wrapping around if end of buffer is reached.
        self.frame_idx = (self.frame_idx + 1) % self.req.frame_number as usize;
        // Determine the location of the buffer which the next frame lies within.
        let buffer_idx = self.frame_idx / frames_per_buffer as usize;
        let buffer_ptr = unsafe {
            self.rx_ring.offset(
                (buffer_idx * self.req.block_size as usize)
                    .try_into()
                    .unwrap(),
            )
        };
        // Determine the location of the frame within that buffer.
        let frame_idx_diff = self.frame_idx % frames_per_buffer;
        self.frame_ptr = unsafe {
            buffer_ptr.offset(
                (frame_idx_diff * self.req.frame_size as usize)
                    .try_into()
                    .unwrap(),
            )
        };
    }
}

impl Drop for UdpCap {
    fn drop(&mut self) {
        // Cleanup the mmap
        unsafe {
            munmap(
                self.rx_ring as *mut c_void,
                (self.req.block_number * self.req.block_size) as usize,
            )
            .expect("Failed un un-mmap the ring buffer")
        };
        close(self.fd).expect("Failed to close socket fd")
    }
}
