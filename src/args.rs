use clap::{Parser, Subcommand};
use regex::Regex;
use std::{net::SocketAddr, ops::RangeInclusive};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Network interface to capture packets on
    #[arg(long)]
    pub cap_interface: String,
    /// Port which we expect packets to be directed to
    #[arg(long, default_value_t = 60000)]
    #[clap(value_parser = clap::value_parser!(u16).range(1..))]
    pub cap_port: u16,
    /// Port which we expect packets to be directed to
    #[arg(long, default_value_t = 65432)]
    #[clap(value_parser = clap::value_parser!(u16).range(1..))]
    pub trig_port: u16,
    /// Port to respond to prometheus requests for metrics
    #[arg(long, default_value_t = 8083)]
    #[clap(value_parser = clap::value_parser!(u16).range(1..))]
    pub metrics_port: u16,
    /// Downsample factor
    #[arg(long, short, default_value_t = 4)]
    pub downsample: u16,
    /// Voltage buffer size as a power of 2
    #[arg(long, short, default_value_t = 22)]
    pub vbuf_power: u32,
    /// CPU cores to which we'll build tasks. They should share a NUMA node.
    #[arg(long, default_value = "8:15", value_parser = parse_core_range)]
    pub core_range: RangeInclusive<usize>,
    /// Socket address of the SNAP Board
    #[arg(long, default_value = "192.168.0.5:69")]
    pub fpga_addr: SocketAddr,
    /// NTP server to synchronize against
    #[arg(long, default_value = "time.google.com")]
    pub ntp_addr: String,
    /// Force a pps trigger
    #[arg(long)]
    pub trig: bool,
    /// Requantization gain
    #[arg(long)]
    pub requant_gain: u32,
    /// Exfil method - leaving this unspecified will not save stokes data
    #[command(subcommand)]
    pub exfil: Option<Exfil>,
}

#[derive(Debug, Subcommand)]
pub enum Exfil {
    /// Use PSRDADA for exfil
    Psrdada {
        /// Hex key
        #[clap(short, long, value_parser = valid_dada_key)]
        key: i32,
        /// Window size
        #[clap(short, long, default_value_t = 65536)]
        samples: usize,
    },
    Filterbank,
}

#[allow(clippy::missing_panics_doc)]
#[allow(clippy::missing_errors_doc)]
pub fn parse_core_range(input: &str) -> Result<RangeInclusive<usize>, String> {
    let re = Regex::new(r"(\d+):(\d+)").unwrap();
    let cap = re.captures(input).unwrap();
    let start: usize = cap[1].parse().unwrap();
    let stop: usize = cap[2].parse().unwrap();
    if stop < start {
        return Err("Invalid CPU range".to_owned());
    }
    if stop - start + 1 < 8 {
        return Err("Not enough CPU cores".to_owned());
    }
    Ok(start..=stop)
}

fn valid_dada_key(s: &str) -> Result<i32, String> {
    i32::from_str_radix(s, 16).map_err(|_| "Invalid hex litteral".to_string())
}
