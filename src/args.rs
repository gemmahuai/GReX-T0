use clap::Parser;

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
    /// Whether to display a TUI
    #[arg(long, short)]
    pub tui: bool,
    /// Downsample factor
    #[arg(long, short, default_value_t = 4)]
    pub downsample: u16,
    /// Voltage buffer size as a power of 2. Defaults to 2^22, or ~34 seconds
    #[arg(long, short, default_value_t = 22)]
    pub vbuf_power: u32,
    /// CPU cores to which we'll build tasks. They should share a NUMA node. Ex: 8:14
    #[arg(long, default_value = "8:14")]
    pub core_range: String,
}

fn parse_core_range(input: &str) -> [usize; 8] {
    todo!()
}
