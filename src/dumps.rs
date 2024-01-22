//! Dumping voltage data

use crate::common::{Payload, BLOCK_TIMEOUT, CHANNELS};
use crate::exfil::{BANDWIDTH, HIGHBAND_MID_FREQ};
use hifitime::prelude::*;
use ndarray::prelude::*;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    str::FromStr,
};
use thingbuf::mpsc::{
    blocking::{Receiver, Sender, StaticReceiver},
    errors::RecvTimeoutError,
};
use tokio::{net::UdpSocket, sync::broadcast};
use tracing::{info, warn};

pub struct DumpRing {
    capacity: usize,
    container: Vec<Payload>,
    write_index: usize,
}

impl DumpRing {
    pub fn next_push(&mut self) -> &mut Payload {
        let before_idx = self.write_index;
        self.write_index = (self.write_index + 1) % (self.capacity - 1);
        &mut self.container[before_idx]
    }

    pub fn new(size_power: u32) -> Self {
        let cap = 2usize.pow(size_power);
        Self {
            container: vec![Payload::default(); cap],
            write_index: 0,
            capacity: cap,
        }
    }

    // Pack the ring into an array of [time, (pol_a, pol_b), channel, (re, im)]
    pub fn dump(&self, start_time: &Epoch, path: &Path) -> eyre::Result<()> {
        // Filename with ISO 8610 standard format
        let fmt = Format::from_str("%Y%m%dT%H%M%S").unwrap();
        let filename = format!("grex_dump-{}.nc", Formatter::new(Epoch::now()?, fmt));
        let file_path = path.join(filename);
        let mut file = netcdf::create(file_path)?;

        // Add the file dimensions
        file.add_dimension("time", self.capacity)?;
        file.add_dimension("pol", 2)?;
        file.add_dimension("freq", CHANNELS)?;
        file.add_dimension("reim", 2)?;

        // Describe the dimensions
        let mut tdb = file.add_variable::<f64>("time", &["time"])?;
        tdb.put_attribute("units", "Days")?;
        tdb.put_attribute(
            "long_name",
            "Days since Dynamic Barycentric Time (TDB) J2000",
        )?;
        // Fill times by traversing the payloads in order
        let mut read_idx = self.write_index;
        let mut idx = 0;
        loop {
            // Get payload ptr
            let pl = self.container.get(read_idx).unwrap();
            tdb.put_value(pl.real_time(start_time).to_tdb_days_since_j2000(), idx)?;
            // Increment the pointers
            idx += 1;
            read_idx = (read_idx + 1) % (self.capacity - 1);
            // Check if we've gone all the way around
            if read_idx == self.write_index {
                break;
            }
        }

        let mut pol = file.add_string_variable("pol", &["pol"])?;
        pol.put_attribute("long_name", "Polarization")?;
        pol.put_string("a", 0)?;
        pol.put_string("b", 1)?;

        let mut freq = file.add_variable::<f64>("freq", &["freq"])?;
        freq.put_attribute("units", "Megahertz")?;
        freq.put_attribute("long_name", "Frequency")?;
        let freqs = Array::linspace(HIGHBAND_MID_FREQ, HIGHBAND_MID_FREQ - BANDWIDTH, CHANNELS);
        freq.put(.., freqs.view())?;

        let mut reim = file.add_string_variable("reim", &["reim"])?;
        reim.put_attribute("long_name", "Complex")?;
        reim.put_string("real", 0)?;
        reim.put_string("imaginary", 1)?;

        // Setup our data block
        let mut voltages = file.add_variable::<i8>("voltages", &["mjd", "pol", "freq", "reim"])?;
        voltages.put_attribute("long_name", "Channelized Voltages")?;
        voltages.put_attribute("units", "Volts")?;

        // Write to the file, one timestep at a time
        idx = 0;
        read_idx = self.write_index;
        loop {
            let pl = self.container.get(read_idx).unwrap();
            voltages.put((idx, .., .., ..), pl.into_ndarray().view())?;
            idx += 1;
            read_idx = (read_idx + 1) % (self.capacity - 1);
            if read_idx == self.write_index {
                break;
            }
        }
        Ok(())
    }
}

pub async fn trigger_task(
    sender: Sender<()>,
    port: u16,
    mut shutdown: broadcast::Receiver<()>,
) -> eyre::Result<()> {
    info!("Starting voltage ringbuffer trigger task!");
    // Create the socket
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let sock = UdpSocket::bind(addr).await?;
    // Maybe even 0 would work, we don't expect data
    let mut buf = [0; 10];
    loop {
        tokio::select! {
            _ = shutdown.recv() => {
                info!("Voltage ringbuffer trigger task stopping");
                break;
            }
            _ = sock.recv_from(&mut buf) => {
                sender.send(())?;
            }
        }
    }
    Ok(())
}

pub fn dump_task(
    mut ring: DumpRing,
    payload_reciever: StaticReceiver<Payload>,
    signal_reciever: Receiver<()>,
    start_time: Epoch,
    path: PathBuf,
    mut shutdown: broadcast::Receiver<()>,
) -> eyre::Result<()> {
    info!("Starting voltage ringbuffer fill task!");
    loop {
        if shutdown.try_recv().is_ok() {
            info!("Dump task stopping");
            break;
        }
        // First check if we need to dump, as that takes priority
        if signal_reciever.try_recv().is_ok() {
            info!("Dumping ringbuffer");
            match ring.dump(&start_time, &path) {
                Ok(_) => (),
                Err(e) => warn!("Error in dumping buffer - {}", e),
            }
        } else {
            // If we're not dumping, we're pushing data into the ringbuffer
            match payload_reciever.recv_ref_timeout(BLOCK_TIMEOUT) {
                Ok(pl) => {
                    let ring_ref = ring.next_push();
                    ring_ref.clone_from(&pl);
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Closed) => break,
                Err(_) => unreachable!(),
            }
        }
    }
    Ok(())
}
