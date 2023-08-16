//! Pre pipeline calibration routine

use crate::fpga::Device;
use eyre::eyre;
use median::Filter;
use tracing::{info, warn};
use whittaker_smoother::whittaker_smoother;

// Around 1 second at 8.192us
const CALIBRATION_ACCUMULATIONS: u32 = 131072;
// Whittaker Settings
const SMOOTH_LAMBDA: f64 = 50.0;
const SMOOTH_ORDER: usize = 3;
// What fraction of the resulting requant byte do we want to scale to
// This determines headroom for RFI
const REQUANT_SCALE: f64 = 0.1;
// Median filter width
const MEDIAN_FILTER_WIDTH: usize = 50;

fn compute_gains(
    scale: f64,
    n: u32,
    powers: &[u64],
    lambda: f64,
    order: usize,
) -> eyre::Result<Vec<u16>> {
    // Compute the mean power (in raw counts)
    // Then convert to average voltage (as power is r^2 + i^2) by sqrt(x/2)
    let norm_volt: Vec<_> = powers
        .iter()
        .map(|x| (*x as f64 / (2.0 * n as f64)).sqrt())
        .collect();
    // Then median filter (in frequency)
    let mut filter = Filter::new(MEDIAN_FILTER_WIDTH);
    let filtered = filter.consume(norm_volt);
    // Smooth the voltage using the whittaker smoother
    let mut smoothed =
        whittaker_smoother(&filtered, lambda, order).ok_or(eyre!("Couldn't smooth"))?;
    // Check to make sure there are no negative numbers or zeros
    for (chan, val) in smoothed.iter_mut().enumerate() {
        if *val <= 0.0 {
            warn!(chan, val, "Curiously small counts from integrated spectrum");
            *val = f64::EPSILON.powi(2);
        }
    }
    // Then invert to scale to a fraction of 2^17 (As that's the binary point of the re and im parts out of the F-engine
    // And multiply by some gain factor <1 to set where the nominal spectrum should line
    // And round to u16 to make it fit in our u16
    let gain: Vec<_> = smoothed
        .into_iter()
        .map(|x| (scale / (x / f64::from(1 << 17))).round() as u16)
        .collect();
    Ok(gain)
}

pub fn calibrate(fpga: &mut Device) -> eyre::Result<()> {
    info!("Calibrating bandpass");
    // Assuming the fpga has been setup (but not adjusted in requant gains),
    // Capture the spectrum
    let (a, b) = fpga.perform_spec_vacc(CALIBRATION_ACCUMULATIONS)?;
    // Compute the gains
    let a_gain = compute_gains(
        REQUANT_SCALE,
        CALIBRATION_ACCUMULATIONS,
        &a,
        SMOOTH_LAMBDA,
        SMOOTH_ORDER,
    )?;
    let b_gain = compute_gains(
        REQUANT_SCALE,
        CALIBRATION_ACCUMULATIONS,
        &b,
        SMOOTH_LAMBDA,
        SMOOTH_ORDER,
    )?;
    fpga.set_requant_gains(&a_gain, &b_gain)?;
    info!("Calibration complete!");
    Ok(())
}
