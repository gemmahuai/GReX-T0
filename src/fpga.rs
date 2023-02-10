//! Control of the SNAP board running the gateware

use anyhow::bail;
use casperfpga::transport::{
    tapcp::{Platform, Tapcp},
    Transport,
};
use casperfpga_derive::fpga_from_fpg;
use chrono::{DateTime, TimeZone, Utc};
use fixed::types::U32F0;
use rsntp::SynchronizationResult;
use std::net::{Ipv4Addr, SocketAddr};

fpga_from_fpg!(GrexFpga, "gateware/grex_gateware_2023-02-09_1739.fpg");

pub struct Device {
    pub fpga: GrexFpga<Tapcp>,
}

impl Device {
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn new(addr: SocketAddr, requant_gain: u32) -> Self {
        let fpga = GrexFpga::new(Tapcp::connect(addr, Platform::SNAP).expect("Connection failed"))
            .expect("Failed to build FPGA object");
        assert!(
            fpga.transport.lock().unwrap().is_running().unwrap(),
            "SNAP board is not programmed/running"
        );
        // Setup gain and requant factors
        fpga.requant_gain
            .write(&U32F0::from_num(requant_gain))
            .unwrap();
        fpga.fft_shift.write(&U32F0::from_num(4095)).unwrap();
        Self { fpga }
    }

    /// Resets the state of the SNAP
    pub fn reset(&mut self) -> anyhow::Result<()> {
        self.fpga.master_rst.write(true);
        self.fpga.master_rst.write(false);
        Ok(())
    }

    /// Gets the 10 GbE data connection in working order
    pub fn start_networking(&mut self) -> anyhow::Result<()> {
        // FIXME, paramaterize
        let dest_ip: Ipv4Addr = "192.168.0.1".parse()?;
        let dest_mac = [0x98, 0xb7, 0x85, 0xa7, 0xec, 0x78];
        let dest_port = 60000u16;
        // Disable
        self.fpga.tx_en.write(false)?;
        self.fpga.gbe1.set_ip("192.168.0.20".parse()?)?;
        self.fpga.gbe1.set_gateway(dest_ip)?;
        self.fpga.gbe1.set_netmask("255.255.255.0".parse()?)?;
        self.fpga.gbe1.set_port(dest_port)?;
        self.fpga
            .gbe1
            .set_mac(&[0x02, 0x2E, 0x46, 0xE0, 0x64, 0xA1])?;
        self.fpga.gbe1.set_enable(true)?;
        self.fpga.gbe1.toggle_reset()?;
        // Set destination registers
        self.fpga.dest_port.write(&U32F0::from_num(dest_port))?;
        self.fpga
            .dest_ip
            .write(&U32F0::from_num(u32::from(dest_ip)))?;
        self.fpga.gbe1.set_single_arp_entry(dest_ip, &dest_mac)?;
        // Turn on the core
        self.fpga.tx_en.write(true)?;
        // Check the link
        if !self.fpga.gbe1_linkup.read()? {
            bail!("10GbE Link Failed to come up");
        }
        Ok(())
    }

    /// Send a trigger pulse to start the flow of bytes, returning the true time of the start of packets
    #[allow(clippy::missing_panics_doc)]
    pub fn trigger(&mut self, time_sync: &SynchronizationResult) -> DateTime<Utc> {
        // Get the current time, and wait to send the triggers to align the time with a rising PPS edge
        let now: DateTime<Utc> = time_sync.datetime().try_into().unwrap();
        // If we wait until halfway through the second, we have the maximum likleyhood of preventing a fencepost error
        let trigger_time = Utc.timestamp_opt(now.timestamp() + 1, 500_000_000).unwrap();
        // PPS will trigger on the next starting edge after we arm
        let start_time = Utc.timestamp_opt(now.timestamp() + 2, 0).unwrap();
        std::thread::sleep((trigger_time - now).to_std().unwrap());
        // Send the trigger
        self.fpga.arm.write(true).unwrap();
        self.fpga.arm.write(false).unwrap();
        // Update our time
        start_time
    }

    /// Force a PPS pulse (timing will be inaccurate)
    #[allow(clippy::missing_panics_doc)]
    pub fn force_pps(&mut self) {
        self.fpga.pps_trig.write(true).unwrap();
        self.fpga.pps_trig.write(false).unwrap();
    }
}
