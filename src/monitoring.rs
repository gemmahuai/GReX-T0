use crate::common::AllChans;
use crossbeam_channel::Receiver;
use pcap::Stat;
use std::time::Instant;

#[allow(clippy::cast_precision_loss)]
#[allow(clippy::similar_names)]
pub fn monitor_task(stat_receiver: &Receiver<Stat>, all_chans: &AllChans) -> ! {
    println!("Starting monitoring task!");
    let mut last_state = Instant::now();
    let mut last_rcv = 0;
    let mut last_drops = 0;
    loop {
        // Blocking here is ok, these are infrequent events
        let stat = stat_receiver.recv().expect("Stat receive channel error");
        let since_last = last_state.elapsed();
        last_state = Instant::now();
        let pps = (stat.received - last_rcv) as f32 / since_last.as_secs_f32();
        let dps = (stat.dropped - last_drops) as f32 / since_last.as_secs_f32();
        last_rcv = stat.received;
        last_drops = stat.dropped;
        println!(
            "{pps} pps\t{dps} dps\t{} pak\t{} stat\t{} stokes\t{}payload",
            all_chans.payload.len(),
            all_chans.stat.len(),
            all_chans.stokes.len(),
            all_chans.payload.len(),
        );
    }
}
