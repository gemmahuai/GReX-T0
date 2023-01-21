use crate::processing::Stokes;
use crossbeam_channel::Receiver;

pub fn dummy_consumer(receiver: Receiver<Stokes>) {
    loop {
        receiver.recv().unwrap();
    }
}
