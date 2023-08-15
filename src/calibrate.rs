//! Pre pipeline calibration routine

use crate::{common::PACKET_CADENCE, fpga::Device};
use eyre::eyre;
use nalgebra::{DMatrix, DVector, RealField, SVD};
use std::{fs, time::Duration};

const CALIBRATION_ACCUMULATIONS: u32 = 131072; // Around 1 second at 8.192us
const FIT_ORDER: usize = 6;

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

pub fn polyfit<T: RealField + Copy>(
    x_values: &[T],
    y_values: &[T],
    polynomial_degree: usize,
) -> eyre::Result<Vec<T>> {
    // Number of terms (x^0 + x^2 ...)
    let cols = polynomial_degree + 1;
    let rows = x_values.len();
    // Setup the linear regression problem
    let mut a = DMatrix::zeros(rows, cols);
    for (row, &x) in x_values.iter().enumerate() {
        // First column is always 1
        a[(row, 0)] = T::one();
        for col in 1..cols {
            a[(row, col)] = x.powf(nalgebra::convert(col as f64));
        }
    }
    // Setup the b side
    let b = DVector::from_row_slice(y_values);
    let decomp = SVD::new(a, true, true);
    match decomp.solve(&b, nalgebra::convert(1e-18f64)) {
        Ok(mat) => Ok(mat.data.into()),
        Err(error) => Err(eyre!(error.to_owned())),
    }
}

pub fn calibrate(fpga: &mut Device) -> eyre::Result<()> {
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
    // Fit to a polynomial
    let channels: Vec<_> = (0..2048).map(|x| x as f64).collect();
    let a_fit = polyfit(&channels, &a_norm, FIT_ORDER)?;
    let b_fit = polyfit(&channels, &b_norm, FIT_ORDER)?;
    // FIXME write to file
    write_to_file(&a_norm, "a");
    write_to_file(&a_fit, "a_fit");
    write_to_file(&b_norm, "b");
    write_to_file(&b_fit, "b_fit");
    Ok(())
}
