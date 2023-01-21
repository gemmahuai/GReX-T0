use crate::common::Payload;
use crossbeam_channel::Receiver;

pub fn dummy_consumer(receiver: Receiver<Payload>) {
    loop {
        receiver.recv().unwrap();
    }
}
