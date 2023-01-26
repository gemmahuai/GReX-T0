use std::{net::SocketAddr, ops::RangeInclusive};

use clap::Parser;
use regex::Regex;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Network interface to capture packets on
    #[arg(long)]
    pub cap_interface: String,
    /// Port which we expect packets to be directed to
    #[arg(long, default_value_t = 60000)]
    pub cap_port: u16,
    /// Port which we expect packets to be directed to
    #[arg(long, default_value_t = 65432)]
    pub trig_port: u16,
    /// Port to respond to prometheus requests for metrics
    #[arg(long, default_value_t = 8083)]
    pub metrics_port: u16,
    /// Downsample factor
    #[arg(long, short, default_value_t = 4)]
    pub downsample: u16,
    /// Voltage buffer size as a power of 2
    #[arg(long, short, default_value_t = 22)]
    pub vbuf_power: u32,
    /// CPU cores to which we'll build tasks. They should share a NUMA node.
    #[arg(long, default_value = "8:15")]
    pub core_range: String,
    /// Socket address of the SNAP Board
    #[arg(long, default_value = "192.168.0.5:69")]
    pub fpga_addr: SocketAddr,
    /// NTP server to synchronize against
    #[arg(long, default_value = "time.google.com")]
    pub ntp_addr: String,
    /// Force a pps trigger
    #[arg(long)]
    pub trig: bool,
}

#[allow(clippy::missing_panics_doc)]
#[must_use]
pub fn parse_core_range(input: &str) -> RangeInclusive<usize> {
    let re = Regex::new(r"(\d+):(\d+)").unwrap();
    let cap = re.captures(input).unwrap();
    let start: usize = cap[1].parse().unwrap();
    let stop: usize = cap[2].parse().unwrap();
    assert!((start > 0) && (stop > start), "Invalid CPU range");
    let range = start..=stop;
    assert!(start - stop + 1 >= 8, "Not enough CPU cores");
    range
}
