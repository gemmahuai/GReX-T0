//! Control of the SNAP board running the gateware

use anyhow::bail;
use casperfpga::transport::{
    tapcp::{Platform, Tapcp},
    Transport,
};
use casperfpga_derive::fpga_from_fpg;
use hifitime::{prelude::*, UNIX_REF_EPOCH};
use rsntp::SynchronizationResult;
use std::net::{Ipv4Addr, SocketAddr};

fpga_from_fpg!(GrexFpga, "gateware/grex_gateware.fpg");

pub struct Device {
    pub fpga: GrexFpga<Tapcp>,
}

impl Device {
    pub fn new(addr: SocketAddr, requant_gain: u32) -> Self {
        let fpga = GrexFpga::new(Tapcp::connect(addr, Platform::SNAP).expect("Connection failed"))
            .expect("Failed to build FPGA object");
        assert!(
            fpga.transport.lock().unwrap().is_running().unwrap(),
            "SNAP board is not programmed/running"
        );
        // Setup gain and requant factors
        fpga.requant_gain.write(requant_gain.into()).unwrap();
        fpga.fft_shift.write(4095u32.into()).unwrap();
        Self { fpga }
    }

    /// Resets the state of the SNAP
    pub fn reset(&mut self) -> anyhow::Result<()> {
        self.fpga.master_rst.write(true)?;
        self.fpga.master_rst.write(false)?;
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
        self.fpga.dest_port.write(dest_port.into())?;
        self.fpga.dest_ip.write(u32::from(dest_ip).into())?;
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
    pub fn trigger(&mut self, time_sync: &SynchronizationResult) -> anyhow::Result<Epoch> {
        // Get the current time, and wait to send the triggers to align the time with a rising PPS edge
        let now = UNIX_REF_EPOCH + hifitime::Duration::from(time_sync.datetime().unix_timestamp()?);
        let next_sec = now.ceil(1.seconds());
        // If we wait a little past the second second, we have the maximum likleyhood of preventing a fencepost error
        let trigger_time = next_sec + 0.1.seconds();
        // PPS will trigger on the next starting edge after we arm
        let start_time = next_sec + 1.seconds();
        std::thread::sleep((trigger_time - now).try_into().unwrap());
        // Send the trigger
        self.fpga.arm.write(true).unwrap();
        self.fpga.arm.write(false).unwrap();
        // Update our time
        Ok(start_time)
    }

    /// Send a trigger pulse to start the flow of bytes, without synchronizing against NTP
    pub fn blind_trigger(&mut self) -> anyhow::Result<Epoch> {
        // Get the current time, and wait to send the triggers to align the time with a rising PPS edge
        let now = hifitime::Epoch::now()?;
        let next_sec = now.ceil(1.seconds());
        // If we wait a little past the second second, we have the maximum likleyhood of preventing a fencepost error
        let trigger_time = next_sec + 0.1.seconds();
        // PPS will trigger on the next starting edge after we arm
        let start_time = next_sec + 1.seconds();
        std::thread::sleep((trigger_time - now).try_into().unwrap());
        // Send the trigger
        self.fpga.arm.write(true).unwrap();
        self.fpga.arm.write(false).unwrap();
        // Update our time
        Ok(start_time)
    }

    /// Force a PPS pulse (timing will be inaccurate)
    #[allow(clippy::missing_panics_doc)]
    pub fn force_pps(&mut self) {
        self.fpga.pps_trig.write(true).unwrap();
        self.fpga.pps_trig.write(false).unwrap();
    }
}
