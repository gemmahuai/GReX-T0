//! Common types shared between tasks

use num_complex::Complex;

/// Number of frequency channels (set by gateware)
pub const CHANNELS: usize = 2048;

#[derive(Debug, Copy, Clone)]
pub struct Payload {
    /// Number of packets since the first packet
    pub count: u64,
    pub pol_a: [Complex<i8>; CHANNELS],
    pub pol_b: [Complex<i8>; CHANNELS],
}

impl Default for Payload {
    fn default() -> Self {
        Self {
            count: Default::default(),
            pol_a: [Complex::new(0, 0); CHANNELS],
            pol_b: [Complex::new(0, 0); CHANNELS],
        }
    }
}
