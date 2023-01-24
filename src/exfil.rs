use crate::common::Stokes;
use crossbeam_channel::Receiver;

#[allow(clippy::missing_panics_doc)]
pub fn dummy_consumer(receiver: &Receiver<Stokes>) {
    loop {
        if receiver.try_recv().is_ok() {
            // nom nom
        }
    }
}
