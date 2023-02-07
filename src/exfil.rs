use crate::common::{Stokes, CHANNELS, PACKET_CADENCE};
use byte_slice_cast::AsByteSlice;
use chrono::{DateTime, Datelike, Timelike, Utc};
use lending_iterator::lending_iterator::LendingIterator;
use log::{debug, info};
use psrdada::client::DadaClient;
use std::sync::{Arc, Mutex};
use std::{collections::HashMap, io::Write};
use thingbuf::mpsc::Receiver;

// Set by hardware (in MHz)
const _LOWBAND_MID_FREQ: f64 = 1_280.061_035_16;
const BANDWIDTH: f64 = 250.0;

/// Convert a chronno `DateTime` into a heimdall-compatible timestamp string
fn heimdall_timestamp(time: &DateTime<Utc>) -> String {
    format!(
        "{}-{:02}-{:02}-{:02}:{:02}:{:02}",
        time.year(),
        time.month(),
        time.day(),
        time.hour(),
        time.minute(),
        time.second()
    )
}

/// Do nothing
#[allow(clippy::missing_panics_doc)]
pub async fn dummy_consumer(stokes_rcv: Receiver<Stokes>) {
    info!("Starting dummy consumer");
    while stokes_rcv.recv().await.is_some() {}
}

// FIXME
// pub async fn dada_consumer(
//     key: i32,
//     stokes_rcv: Receiver<Stokes>,
//     payload_start: DateTime<Utc>,
//     window_size: usize,
// ) -> anyhow::Result<()> {
//     // DADA window
//     let mut stokes_cnt = 0usize;
//     // We will capture the timestamp on the first packet
//     let mut first_payload = true;
//     // Send the header (heimdall only wants one)
//     let mut header = HashMap::from([
//         ("NCHAN".to_owned(), CHANNELS.to_string()),
//         ("BW".to_owned(), BANDWIDTH.to_string()),
//         ("FREQ".to_owned(), "1405".to_owned()),
//         ("NPOL".to_owned(), "1".to_owned()),
//         ("NBIT".to_owned(), "32".to_owned()),
//         ("OBS_OFFSET".to_owned(), 0.to_string()),
//         ("TSAMP".to_owned(), (PACKET_CADENCE * 1e6).to_string()), // FIXME, downsample?
//     ]);
//     // Grab PSRDADA writing context
//     let mut client = DadaClient::new(key).expect("Could not connect to PSRDADA buffer");
//     let (mut hc, mut dc) = client.split();
//     let mut data_writer = dc.writer();
//     info!("DADA header pushed, starting exfil to Heimdall");
//     // Start the main consumer loop
//     loop {
//         // Grab the next psrdada block we can write to (BLOCKING)
//         let mut block = tokio::task::spawn_blocking(move || data_writer.next()).await?;
//         loop {
//             // Grab the next stokes parameters (already downsampled)
//             let mut stokes = stokes_rcv.recv().await?;
//             // Timestamp first one
//             if first_payload {
//                 first_payload = false;
//                 // The first payload we recieve will be payload #1 (as we armed and triggered)
//                 let timestamp_str = heimdall_timestamp(payload_start);
//                 header.insert("UTC_START".to_owned(), timestamp_str);
//                 // Write the single header
//                 // Safety: All these header keys and values are valid
//                 unsafe { hc.push_header(&header).unwrap() };
//             }
//             // Zero the first and last 250 sample to remove the aliasing artifacts from the edges
//             stokes[0..=250].fill(0.0);
//             stokes[1797..=2047].fill(0.0);
//             // Write the block
//             block.write_all(stokes.as_byte_slice()).unwrap();
//             // Increase our count
//             stokes_cnt += 1;
//             // If we've filled the window, commit it to PSRDADA
//             if stokes_cnt == window_size {
//                 debug!("Commiting window to PSRDADA");
//                 // Reset the stokes counter
//                 stokes_cnt = 0;
//                 // Commit data and update
//                 block.commit();
//                 //Break to finish the write
//                 break;
//             }
//         }
//     }
// }
