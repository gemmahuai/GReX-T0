//! Common types shared between tasks

use crossbeam_channel::Sender;
use num_complex::Complex;
use pcap::Stat;

/// Number of frequency channels (set by gateware)
pub const CHANNELS: usize = 2048;

pub type Stokes = [f32; CHANNELS];

#[derive(Debug, Copy, Clone)]
pub struct Payload {
    /// Number of packets since the first packet
    pub count: u64,
    pub pol_a: [Complex<i8>; CHANNELS],
    pub pol_b: [Complex<i8>; CHANNELS],
}

#[derive(Debug)]
pub struct AllChans {
    pub payload: Sender<Payload>,
    pub stat: Sender<Stat>,
    pub stokes: Sender<Stokes>,
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

impl Payload {
    /// Calculate the Stokes-I parameter for this payload
    #[allow(clippy::missing_panics_doc)]
    #[allow(clippy::cast_sign_loss)]
    #[must_use]
    pub fn stokes_i(&self) -> [u16; CHANNELS] {
        let mut stokes = [0u16; CHANNELS];
        for (i, (a, b)) in self.pol_a.into_iter().zip(self.pol_b).enumerate() {
            stokes[i] =
                ((i16::from(a.re) * i16::from(a.im)) + (i16::from(b.re) * i16::from(b.im))) as u16;
        }
        stokes
    }
}
