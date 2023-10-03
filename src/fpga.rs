//! Control of the SNAP board running the gateware
use casperfpga::transport::{
    tapcp::{Platform, Tapcp},
    Transport,
};
use casperfpga_derive::fpga_from_fpg;
use eyre::bail;
use fixed::{types::extra::U0, FixedU16};
use hifitime::{prelude::*, UNIX_REF_EPOCH};
use rsntp::SynchronizationResult;
use std::net::{Ipv4Addr, SocketAddr};
use tracing::debug;

use crate::common::PACKET_CADENCE;

fpga_from_fpg!(GrexFpga, "gateware/grex_gateware.fpg");

pub struct Device {
    pub fpga: GrexFpga<Tapcp>,
}

impl Device {
    pub fn new(addr: SocketAddr) -> Self {
        let fpga = GrexFpga::new(Tapcp::connect(addr, Platform::SNAP).expect("Connection failed"))
            .expect("Failed to build FPGA object");
        assert!(
            fpga.transport.lock().unwrap().is_running().unwrap(),
            "SNAP board is not programmed/running"
        );
        fpga.fft_shift.write(4095u32.into()).unwrap();
        Self { fpga }
    }

    /// Resets the state of the SNAP
    pub fn reset(&mut self) -> eyre::Result<()> {
        self.fpga.master_rst.write(true)?;
        self.fpga.master_rst.write(false)?;
        Ok(())
    }

    /// Gets the 10 GbE data connection in working order
    pub fn start_networking(&mut self) -> eyre::Result<()> {
        let dest_ip: Ipv4Addr = "192.168.0.1".parse()?;
        let dest_port = 60000u16;
        // Disable
        self.fpga.tx_en.write(false)?;
        self.fpga.gbe1.set_ip("192.168.0.20".parse()?)?;
        self.fpga.gbe1.set_gateway(dest_ip)?;
        self.fpga.gbe1.set_netmask("255.255.255.0".parse()?)?;
        self.fpga.gbe1.set_port(dest_port)?;
        // Fixed in gateware
        self.fpga
            .gbe1
            .set_mac(&[0x02, 0x2E, 0x46, 0xE0, 0x64, 0xA1])?;
        self.fpga.gbe1.set_enable(true)?;
        self.fpga.gbe1.toggle_reset()?;
        // Set destination registers
        self.fpga.dest_port.write(dest_port.into())?;
        self.fpga.dest_ip.write(u32::from(dest_ip).into())?;
        self.fpga
            .gbe1
            .set_single_arp_entry(dest_ip, &[0, 0, 0, 0, 0, 0])?;
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
    pub fn trigger(&mut self, time_sync: &SynchronizationResult) -> eyre::Result<Epoch> {
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
    pub fn blind_trigger(&mut self) -> eyre::Result<Epoch> {
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
    pub fn force_pps(&mut self) -> eyre::Result<()> {
        self.fpga.pps_trig.write(true)?;
        self.fpga.pps_trig.write(false)?;
        Ok(())
    }

    /// Trigger, wait, and read spectrum VACC,
    /// reinterpreting fixed point to bits
    pub fn perform_spec_vacc(&mut self, n: u32) -> eyre::Result<(Vec<u64>, Vec<u64>)> {
        // Set the number of accumulations
        self.fpga.spec_vacc_n.write(n.into())?;
        // Trigger a pre-requant accumulation
        self.trigger_spec_vacc()?;
        // Wait for the accumulation to complete (plus a little extra wiggle room)
        std::thread::sleep(std::time::Duration::from_secs_f64(
            2.0 * n as f64 * PACKET_CADENCE,
        ));
        // Then capture the spectrum
        let (a, b) = self.read_spec_vacc()?;
        // And return!
        Ok((a, b))
    }

    /// Trigger, wait, and read stokes VACC,
    /// reinterpreting fixed point to bits
    pub fn perform_stokes_vacc(&mut self, n: u32) -> eyre::Result<Vec<u64>> {
        // Set the number of accumulations
        self.fpga.stokes_vacc_n.write(n.into())?;
        // Trigger an accumulation
        self.trigger_stokes_vacc()?;
        // Wait for the accumulation to complete (plus a little extra wiggle room)
        std::thread::sleep(std::time::Duration::from_secs_f64(
            2.0 * n as f64 * PACKET_CADENCE,
        ));
        // Then capture the spectrum
        let stokes = self.read_stokes_vacc()?;
        // And return!
        Ok(stokes)
    }

    /// Trigger and wait for both vaccs simultaneously
    pub fn perform_both_vacc(&mut self, n: u32) -> eyre::Result<(Vec<u64>, Vec<u64>, Vec<u64>)> {
        // Set the number of accumulations
        self.fpga.stokes_vacc_n.write(n.into())?;
        self.fpga.spec_vacc_n.write(n.into())?;
        // Trigger a pre-requant accumulation
        self.trigger_stokes_vacc()?;
        self.trigger_spec_vacc()?;
        std::thread::sleep(std::time::Duration::from_secs_f64(
            2.0 * n as f64 * PACKET_CADENCE,
        ));
        // Then capture the data
        let stokes = self.read_stokes_vacc()?;
        let (a, b) = self.read_spec_vacc()?;
        Ok((a, b, stokes))
    }

    /// Trigger a pre-requant vector accumulation
    fn trigger_spec_vacc(&mut self) -> eyre::Result<()> {
        self.fpga.spec_vacc_trig.write(true)?;
        self.fpga.spec_vacc_trig.write(false)?;
        Ok(())
    }

    /// Trigger a stokes accumulation
    fn trigger_stokes_vacc(&mut self) -> eyre::Result<()> {
        self.fpga.stokes_vacc_trig.write(true)?;
        self.fpga.stokes_vacc_trig.write(false)?;
        Ok(())
    }

    /// Read both vector accumulations from the spectrum vacc
    fn read_spec_vacc(&mut self) -> eyre::Result<(Vec<u64>, Vec<u64>)> {
        // Read the spectra
        let a = self.fpga.spec_a_vacc.read()?;
        let b = self.fpga.spec_b_vacc.read()?;
        let a_cast = a.iter().map(|v| v.to_bits()).collect();
        let b_cast = b.iter().map(|v| v.to_bits()).collect();
        Ok((a_cast, b_cast))
    }

    /// Read stokes vacc
    fn read_stokes_vacc(&mut self) -> eyre::Result<Vec<u64>> {
        // Read the spectra
        let stokes = self.fpga.stokes_vacc.read()?;
        let stokes_cast = stokes.iter().map(|v| v.to_bits()).collect();
        Ok(stokes_cast)
    }

    pub fn set_requant_gains(&mut self, a: &[u16], b: &[u16]) -> eyre::Result<()> {
        // Cast
        let a_fixed: Vec<_> = a.iter().map(|x| FixedU16::<U0>::from_num(*x)).collect();
        let b_fixed: Vec<_> = b.iter().map(|x| FixedU16::<U0>::from_num(*x)).collect();
        self.fpga.requant_gains_a.write(&a_fixed)?;
        self.fpga.requant_gains_b.write(&b_fixed)?;
        Ok(())
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        debug!("Cleaning up SNAP");
        let _ = self.reset();
    }
}
