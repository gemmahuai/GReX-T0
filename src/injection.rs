//! Task for injecting a fake pulse into the timestream to test/validate downstream components

use crate::common::Stokes;
use thingbuf::mpsc::blocking::{Receiver, Sender};

pub fn pulse_injection_task(_input: Receiver<Stokes>, _output: Sender<Stokes>) {
    todo!()
}
