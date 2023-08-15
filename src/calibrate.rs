//! Pre pipeline calibration routine

use crate::{common::PACKET_CADENCE, fpga::Device};
use eyre::eyre;
use std::time::Duration;
use tracing::info;
use whittaker_smoother::whittaker_smoother;

const CALIBRATION_ACCUMULATIONS: u32 = 131072; // Around 1 second at 8.192us
const SMOOTH_LAMBDA: f64 = 50.0;
const SMOOTH_ORDER: usize = 3;
const REQUANT_SCALE: f64 = 8.0;

// fn write_to_file(data: &[f64], filename: &str) {
//     fs::write(
//         filename,
//         data.iter()
//             .map(|v| v.to_string())
//             .collect::<Vec<_>>()
//             .join("\n"),
//     )
//     .unwrap();
// }

pub fn calibrate(fpga: &mut Device) -> eyre::Result<()> {
    info!("Calibrating bandpass");
    // Assuming the fpga has been setup (but not adjusted in requant gains),
    // Set the number of accumulations
    fpga.set_acc_n(CALIBRATION_ACCUMULATIONS)?;
    // Trigger a pre-requant accumulation
    fpga.trigger_vacc()?;
    // Wait for the accumulation to complete
    std::thread::sleep(Duration::from_secs_f64(
        CALIBRATION_ACCUMULATIONS as f64 * PACKET_CADENCE,
    ));
    // Then capture the spectrum
    let (a, b) = fpga.read_vacc()?;
    // And find the mean by dividing by N
    let a_norm: Vec<_> = a
        .into_iter()
        .map(|x| x as f64 / CALIBRATION_ACCUMULATIONS as f64)
        .collect();
    let b_norm: Vec<_> = b
        .into_iter()
        .map(|x| x as f64 / CALIBRATION_ACCUMULATIONS as f64)
        .collect();
    // Smooth the data out
    let a_smoothed =
        whittaker_smoother(&a_norm, SMOOTH_LAMBDA, SMOOTH_ORDER).ok_or(eyre!("Couldn't smooth"))?;
    let b_smoothed =
        whittaker_smoother(&b_norm, SMOOTH_LAMBDA, SMOOTH_ORDER).ok_or(eyre!("Couldn't smooth"))?;
    // Find the first scaling factor by finding the max smoothed (ignoring the DC term)
    let a_max = *a_smoothed[10..]
        .iter()
        .max_by(|a, b| a.total_cmp(b))
        .unwrap();
    let b_max = *b_smoothed[10..]
        .iter()
        .max_by(|a, b| a.total_cmp(b))
        .unwrap();
    // Convert to "gains" by taking their inverse and scaling
    let a_gain: Vec<_> = a_smoothed
        .into_iter()
        .map(|x| (1.0 / x) * a_max * REQUANT_SCALE)
        .collect();
    let b_gain: Vec<_> = b_smoothed
        .into_iter()
        .map(|x| (1.0 / x) * b_max * REQUANT_SCALE)
        .collect();
    // And set
    let a_gain_int: Vec<_> = a_gain.into_iter().map(|x| x.round() as u16).collect();
    let b_gain_int: Vec<_> = b_gain.into_iter().map(|x| x.round() as u16).collect();
    fpga.set_requant_gains(&a_gain_int, &b_gain_int)?;
    // FIXME write to file
    // write_to_file(&a_norm, "a");
    // write_to_file(&a_smoothed, "a_smoothed");
    // write_to_file(&b_norm, "b");
    // write_to_file(&b_smoothed, "b_smoothed");
    info!("Calibration complete!");
    Ok(())
}
