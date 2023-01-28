use crate::common::{AllChans, CHANNELS};
use crate::fpga::Device;
use crossbeam::channel::Receiver;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::header::CONTENT_TYPE;
use hyper::http::HeaderValue;
use hyper::{Request, Response};
use lazy_static::lazy_static;
use log::{info, warn};
use pcap::Stat;
use prometheus::{
    register_gauge, register_gauge_vec, register_int_gauge, register_int_gauge_vec, Encoder, Gauge,
    GaugeVec, IntGauge, IntGaugeVec, TextEncoder,
};
use std::convert::Infallible;
use std::time::Duration;

lazy_static! {
    static ref CHANNEL_GAUGE: IntGaugeVec = register_int_gauge_vec!(
        "task_channel_backlog",
        "Number of yet-to-be-processed data in each inter-task channel",
        &["target_channel"]
    )
    .unwrap();
    static ref SPECTRUM_GAUGE: GaugeVec =
        register_gauge_vec!("spectrum", "Average spectrum data", &["channel"]).unwrap();
    static ref PPS_GAUGE: Gauge =
        register_gauge!("pps", "Number of packets we're processing per second").unwrap();
    static ref DPS_GAUGE: Gauge =
        register_gauge!("dps", "Number of packets we're dropping per second").unwrap();
    static ref FFT_OVFL_GAUGE: IntGauge =
        register_int_gauge!("fft_ovfl", "Counter of FFT overflows").unwrap();
    static ref REQUANT_OVFL_GAUGE: IntGauge =
        register_int_gauge!("requant_ovfl", "Counter of requantization overflows").unwrap();
    static ref FPGA_TEMP: Gauge =
        register_gauge!("fpga_temp", "Internal FPGA temperature").unwrap();
}

#[allow(clippy::missing_panics_doc)]
#[allow(clippy::missing_errors_doc)]
#[allow(clippy::unused_async)]
pub async fn metrics(
    _: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, Infallible> {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let body_str = encoder.encode_to_string(&metric_families).unwrap();
    let mut resp = Response::new(Full::new(body_str.into()));
    resp.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_str(encoder.format_type()).unwrap(),
    );
    Ok(resp)
}

#[allow(clippy::cast_precision_loss)]
#[allow(clippy::similar_names)]
#[allow(clippy::missing_panics_doc)]
pub fn monitor_task(
    stat_receiver: &Receiver<(Stat, Duration)>,
    spec_rcv: &Receiver<[f64; CHANNELS]>,
    all_chans: &AllChans,
    device: &Device,
) -> ! {
    info!("Starting monitoring task!");
    let mut last_rcv = 0;
    let mut last_drops = 0;
    loop {
        // Blocking here is ok, these are infrequent events
        let (stat, dur) = stat_receiver.recv().unwrap();

        // Then wait for spectrum
        let avg_spec = spec_rcv.recv().unwrap();

        // Process packet stats
        let pps = (stat.received - last_rcv) as f32 / dur.as_secs_f32();
        let dps = (stat.dropped - last_drops) as f32 / dur.as_secs_f32();
        last_rcv = stat.received;
        last_drops = stat.dropped;

        // Metrics from the FPGA
        if let Ok(v) = device.fpga.fft_overflow_cnt.read() {
            FFT_OVFL_GAUGE.set(u32::from(v).try_into().unwrap());
        } else {
            warn!("Error reading from FPGA");
        }
        if let Ok(v) = device.fpga.clip_cnt.read() {
            REQUANT_OVFL_GAUGE.set(u32::from(v).try_into().unwrap());
        } else {
            warn!("Error reading from FPGA");
        }
        if let Ok(v) = device.fpga.transport.lock().unwrap().temperature() {
            FPGA_TEMP.set(v.try_into().unwrap());
        } else {
            warn!("Error reading from FPGA");
        }

        // Update metrics
        PPS_GAUGE.set(pps.into());
        DPS_GAUGE.set(dps.into());
        CHANNEL_GAUGE
            .with_label_values(&["to_split"])
            .set(all_chans.cap_payload.len().try_into().unwrap());
        CHANNEL_GAUGE
            .with_label_values(&["to_downsample"])
            .set(all_chans.payload_to_downsample.len().try_into().unwrap());
        CHANNEL_GAUGE
            .with_label_values(&["to_dump"])
            .set(all_chans.payload_to_ring.len().try_into().unwrap());
        CHANNEL_GAUGE
            .with_label_values(&["to_exfil"])
            .set(all_chans.stokes.len().try_into().unwrap());

        // Update channel data
        for (i, v) in avg_spec.into_iter().enumerate() {
            SPECTRUM_GAUGE.with_label_values(&[&i.to_string()]).set(v);
        }
    }
}
