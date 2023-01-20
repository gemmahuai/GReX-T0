use crate::if_packet::*;
use nix::{
    libc::{setsockopt, sockaddr_ll, SOL_PACKET},
    sys::{
        mman::{mmap, munmap, MapFlags, ProtFlags},
        socket::{socket, AddressFamily, SockFlag, SockProtocol, SockType},
    },
    unistd::{close, sysconf, SysconfVar},
};
use std::{
    ffi::{c_uint, c_void},
    mem::size_of,
    num::NonZeroUsize,
    os::fd::RawFd,
};

/// The number of frames in the ring
const CONF_RING_FRAMES: usize = 128;
const ETH_HLEN: u32 = 14;
const fn tpacket_align(x: u32) -> u32 {
    ((x) + TPACKET_ALIGNMENT - 1) & !(TPACKET_ALIGNMENT - 1)
}
const TPACKET3_HDRLEN: u32 =
    tpacket_align(size_of::<tpacket3_hdr>() as u32) + size_of::<sockaddr_ll>() as u32;

pub struct UdpCap {
    fd: RawFd,
    req: tpacket_req3,
    frame_idx: usize,
    frame_ptr: *mut u8,
    rx_ring: *mut u8,
}

impl UdpCap {
    pub fn new(mtu: usize) -> Self {
        // Create the AF_PACKET socket file descriptor
        let fd = socket(
            AddressFamily::Packet,
            SockType::Datagram,
            SockFlag::empty(),
            SockProtocol::Udp,
        )
        .expect("Creating the raw socket failed");
        // Set the socket option to use TPACKET_V3
        let v = tpacket_versions_TPACKET_V3;
        if unsafe {
            setsockopt(
                fd,
                SOL_PACKET,
                PACKET_VERSION.try_into().unwrap(),
                &v as *const _ as *const c_void,
                size_of::<u32>() as u32,
            )
        } == -1
        {
            panic!(
                "Setting the PACKET_VERSION to TPACKET_V3 failed: {:#?}",
                std::io::Error::last_os_error()
            );
        }

        // Frames will be header + MTU
        let frame_size =
            (tpacket_align(TPACKET3_HDRLEN + ETH_HLEN) + tpacket_align(mtu as u32)) as c_uint;
        // Blocks are the smallest power of two multiple of the page size that is at least the frame size
        let page_size = sysconf(SysconfVar::PAGE_SIZE)
            .expect("Unable to get the page size")
            .unwrap() as u32;
        let block_size = (frame_size / page_size + 1).next_power_of_two() * page_size;
        let block_number = CONF_RING_FRAMES as u32;
        let frame_number = (block_number * block_size) / frame_size;

        // Start to set up the ring buffer request
        let req = tpacket_req3 {
            tp_block_size: block_size,
            tp_block_nr: block_number,
            tp_frame_size: frame_size,
            tp_frame_nr: frame_number,
            tp_retire_blk_tov: 60,
            tp_sizeof_priv: 0,
            tp_feature_req_word: TP_FT_REQ_FILL_RXHASH,
        };

        dbg!(&req);

        // Safety: FD is valid because we've used a safe API, req exists, and we're checking the return
        if unsafe {
            setsockopt(
                fd,
                SOL_PACKET,
                PACKET_RX_RING.try_into().unwrap(),
                &req as *const _ as *const c_void,
                size_of::<tpacket_req3>() as u32,
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
                NonZeroUsize::new((block_number * block_size) as usize).unwrap(),
                ProtFlags::PROT_READ | ProtFlags::PROT_WRITE,
                MapFlags::MAP_SHARED | MapFlags::MAP_LOCKED,
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
}

impl Drop for UdpCap {
    fn drop(&mut self) {
        // Cleanup the mmap
        unsafe {
            munmap(
                self.rx_ring as *mut c_void,
                (self.req.tp_block_nr * self.req.tp_block_size) as usize,
            )
            .expect("Failed un un-mmap the ring buffer")
        };
        // Close the socket (although this might also close the mmap, not sure)
        close(self.fd).expect("Failed to close socket fd")
    }
}
