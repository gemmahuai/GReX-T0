//! Pre pipeline calibration routine

use crate::{common::PACKET_CADENCE, fpga::Device};
use eyre::eyre;
use std::{fs, time::Duration};
use tracing::info;
use whittaker_smoother::whittaker_smoother;

const CALIBRATION_ACCUMULATIONS: u32 = 131072; // Around 1 second at 8.192us
const SMOOTH_LAMBDA: f64 = 25.0;
const SMOOTH_ORDER: usize = 3;

fn write_to_file(data: &[f64], filename: &str) {
    fs::write(
        filename,
        data.iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
}

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
    // FIXME write to file
    write_to_file(&a_norm, "a");
    write_to_file(&a_smoothed, "a_smoothed");
    write_to_file(&b_norm, "b");
    write_to_file(&b_smoothed, "b_smoothed");
    info!("Calibration complete!");
    Ok(())
}
