use crate::capture::Payload;
use crossbeam_channel::Receiver;

pub fn downsample_task(receiver: Receiver<Payload>, downsample_factor: usize) {
    println!("Starting downsample task");
    loop {
        receiver.recv().unwrap();
    }
}
