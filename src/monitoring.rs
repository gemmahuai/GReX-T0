use crossbeam_channel::Receiver;
use pcap::Stat;

pub fn monitor_task(stat_receiver: Receiver<Stat>) -> ! {
    println!("Starting monitoring task!");
    loop {
        let stat = stat_receiver.recv().expect("Stat receive channel error");
        println!("{:#?}", stat);
    }
}
