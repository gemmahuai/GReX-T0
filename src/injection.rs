//! Task for injecting a fake pulse into the timestream to test/validate downstream components

use crate::common::Stokes;
use crossbeam_channel::{Receiver, Sender};

pub fn pulse_injection_task(input: Receiver<Stokes>, output: Sender<Stokes>) {
    todo!()
}
