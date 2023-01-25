use casperfpga::transport::{
    tapcp::{Platform, Tapcp},
    Transport,
};
use casperfpga_derive::fpga_from_fpg;
use chrono::{DateTime, Utc};
use std::net::SocketAddr;

fpga_from_fpg!(GrexFpga, "gateware/grex_gateware_2022-11-09_2251.fpg");

struct Device {
    fpga: GrexFpga<Tapcp>,
    first_packet: Option<DateTime<Utc>>,
}

impl Device {
    pub fn new(addr: SocketAddr) -> Self {
        let mut fpga =
            GrexFpga::new(Tapcp::connect(addr, Platform::SNAP).expect("Connection failed"))
                .expect("Failed to build FPGA object");
        assert!(
            fpga.transport.lock().unwrap().is_running().unwrap(),
            "SNAP board is not programmed/running"
        );
        Self {
            fpga,
            first_packet: None,
        }
    }

    /// Send a trigger pulse to start the flow of bytes
    pub fn trigger(&mut self) {
        self.fpga.master_rst.write(false).unwrap();
        self.fpga.master_rst.write(true).unwrap();
        self.fpga.master_rst.write(false).unwrap();
    }
}
