use crate::processing::Stokes;
use crossbeam_channel::Receiver;

#[allow(clippy::missing_panics_doc)]
pub fn dummy_consumer(receiver: &Receiver<Stokes>) {
    loop {
        receiver.recv().unwrap();
    }
}
