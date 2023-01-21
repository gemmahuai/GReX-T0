use clap::Parser;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    /// Network interface to capture packets on
    #[arg(long)]
    pub cap_interface: String,
    /// Port which we expect packets to be directed to
    #[arg(long)]
    pub cap_port: u16,
    /// Whether to display a TUI
    #[arg(long, short)]
    pub tui: bool,
    /// Downsample factor
    #[arg(long, short, default_value_t = 16)]
    pub downsample: u16,
    /// Voltage buffer size. Defaults to ~4.3 seconds
    #[arg(long, short, default_value_t = 524288)]
    pub vbuf_size: usize,
}
