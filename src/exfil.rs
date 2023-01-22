use crate::common::Stokes;
use crossbeam_channel::Receiver;

#[allow(clippy::missing_panics_doc)]
pub fn dummy_consumer(receiver: &Receiver<Stokes>) {
    loop {
        // High speed, busy wait
        match receiver.try_recv() {
            Ok(_) => (),
            Err(_) => (),
        }
    }
}
