//! Common types shared between tasks

use core::num;

use chrono::{DateTime, Utc};
use crossbeam::channel::{Receiver, Sender};
use ndarray::{s, Array3, ArrayView};
use num_complex::Complex;

/// Number of frequency channels (set by gateware)
pub const CHANNELS: usize = 2048;
/// How sure are we?
pub const PACKET_CADENCE: f64 = 8.192e-6;

pub type Stokes = [f32; CHANNELS];

#[derive(Debug, Clone)]
pub struct Payload {
    /// Number of packets since the first packet
    pub count: u64,
    pub pol_a: [Complex<i8>; CHANNELS],
    pub pol_b: [Complex<i8>; CHANNELS],
}

#[derive(Debug)]
pub struct AllChans {
    pub cap_payload: Receiver<Payload>,
    pub payload_to_downsample: Receiver<Payload>,
    pub payload_to_ring: Receiver<Payload>,
    pub stokes: Receiver<Stokes>,
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
    pub fn stokes_i(&self) -> [f32; CHANNELS] {
        let i8_max = f32::from(i8::MAX);
        let mut stokes = [0.0; CHANNELS];

        for (i, (a, b)) in self.pol_a.into_iter().zip(self.pol_b).enumerate() {
            let a_re = f32::from(a.re) / i8_max;
            let a_im = f32::from(a.im) / i8_max;
            let b_re = f32::from(b.re) / i8_max;
            let b_im = f32::from(b.im) / i8_max;
            stokes[i] = a_re * a_re + a_im * a_im + b_re * b_re + b_im * b_im;
        }
        stokes
    }

    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn packed_pols(&self) -> (&[i8], &[i8]) {
        // # Safety
        // - Data is valid for reads of len as each pol has exactly CHANNELS * 2 bytes
        // - and is contiguous as per the spec of Complex<i8>
        // - Data is initialized at this point as Self has been constructed
        // - Data will not be mutated as this function takes an immutable borrow
        // - Total len is smaller than isize::MAX
        let bytes_a = unsafe {
            std::slice::from_raw_parts(
                std::ptr::addr_of!(self.pol_a).cast::<i8>(),
                CHANNELS * 2, // Real + Im for each element
            )
        };
        let bytes_b = unsafe {
            std::slice::from_raw_parts(
                std::ptr::addr_of!(self.pol_b).cast::<i8>(),
                CHANNELS * 2, // Real + Im for each element
            )
        };
        (bytes_a, bytes_b)
    }

    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    // ndarray of size [(pol_a,pol_b), CHANNELS, (re,im)]
    pub fn into_ndarray(&self) -> Array3<i8> {
        let mut buf = Array3::zeros((2, CHANNELS, 2));
        let (a, b) = self.packed_pols();
        let a = ArrayView::from_shape((CHANNELS, 2), a).expect("Failed to make array view");
        let b = ArrayView::from_shape((CHANNELS, 2), b).expect("Failed to make array view");
        // And assign
        buf.slice_mut(s![0, .., ..]).assign(&a);
        buf.slice_mut(s![1, .., ..]).assign(&b);
        buf
    }

    #[allow(clippy::missing_panics_doc)]
    #[allow(clippy::cast_precision_loss)]
    #[must_use]
    /// Return the real UTC time of this packet
    pub fn real_time(&self, start_time: &DateTime<Utc>) -> DateTime<Utc> {
        let second_offset = self.count as f64 * PACKET_CADENCE;
        *start_time
            + chrono::Duration::from_std(std::time::Duration::from_secs_f64(second_offset)).unwrap()
    }
}

// Splits a channel with a clone
#[allow(clippy::missing_panics_doc)]
pub fn payload_split(
    rcv: &Receiver<Payload>,
    to_downsample: &Sender<Payload>,
    to_dumps: &Sender<Payload>,
) {
    loop {
        let x = rcv.recv().unwrap();
        // This should block if we get held up
        to_downsample.send(x.clone()).unwrap();
        // This one won't cause backpressure because that only will happen when we're doing IO
        let _ = to_dumps.try_send(x);
    }
}
