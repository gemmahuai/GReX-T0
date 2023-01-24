use crate::common::Stokes;
use crossbeam::channel::Receiver;

#[allow(clippy::missing_panics_doc)]
pub fn dummy_consumer(receiver: &Receiver<Stokes>) {
    loop {
        receiver.recv().unwrap();
    }
}
