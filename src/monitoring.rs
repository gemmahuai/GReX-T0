use crate::common::AllChans;
use crossbeam::channel::Receiver;
use log::info;
use pcap::Stat;
use std::time::Instant;

#[allow(clippy::cast_precision_loss)]
#[allow(clippy::similar_names)]
pub fn monitor_task(stat_receiver: &Receiver<Stat>, all_chans: &AllChans) -> ! {
    info!("Starting monitoring task!");
    let mut last_state = Instant::now();
    let mut last_rcv = 0;
    let mut last_drops = 0;
    let mut header = false;
    loop {
        // Blocking here is ok, these are infrequent events
        let stat = stat_receiver.recv().expect("Stat receive channel error");
        let since_last = last_state.elapsed();
        last_state = Instant::now();
        let pps = (stat.received - last_rcv) as f32 / since_last.as_secs_f32();
        let dps = (stat.dropped - last_drops) as f32 / since_last.as_secs_f32();
        last_rcv = stat.received;
        last_drops = stat.dropped;
        if !header {
            println!(
                "{0: <10} | {1: <10} | {2: <10} | {3: <10} | {4: <10} | {5: <10}",
                "PPS", "DPS", "->Split", "->Downsamp", "->Ring", "->Stokes"
            );
            header = true;
        }
        println!(
            "{0: <10} | {1: <10} | {2: <10} | {3: <10} | {4: <10} | {5: <10}",
            pps,
            dps,
            all_chans.cap_payload.len(),
            all_chans.payload_to_downsample.len(),
            all_chans.payload_to_ring.len(),
            all_chans.stokes.len(),
        );
    }
}
