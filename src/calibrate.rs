//! Pre pipeline calibration routine

use crate::fpga::Device;
use std::fs;

fn write_to_file(data: &[u64], filename: &str) {
    fs::write(
        filename,
        data.iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
}

pub fn calibrate(fpga: &mut Device) {
    // Assuming the fpga has been setup (but not adjusted in requant gains),
    // Trigger a pre-requant accumulation
    fpga.trigger_vacc();
    // Then capture the spectrum
    let (a, b) = fpga.read_vacc();
    // FIXME write to file
    write_to_file(&a, "a");
    write_to_file(&b, "b");
}
