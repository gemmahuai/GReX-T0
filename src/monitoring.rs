use crate::common::AllChans;
use crossbeam::channel::Receiver;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::header::CONTENT_TYPE;
use hyper::http::HeaderValue;
use hyper::{Request, Response};
use lazy_static::lazy_static;
use log::info;
use pcap::Stat;
use prometheus::{
    register_gauge, register_int_gauge_vec, Encoder, Gauge, IntGaugeVec, TextEncoder,
};
use std::convert::Infallible;
use std::time::Instant;

lazy_static! {
    static ref CHANNEL_GAUGE: IntGaugeVec = register_int_gauge_vec!(
        "task_channel_backlog",
        "Number of yet-to-be-processed data in each inter-task channel",
        &["target_channel"]
    )
    .unwrap();
    static ref PPS_GAUGE: Gauge =
        register_gauge!("pps", "Number of packets we're processing per second").unwrap();
    static ref DPS_GAUGE: Gauge =
        register_gauge!("dps", "Number of packets we're dropping per second").unwrap();
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
pub fn monitor_task(stat_receiver: &Receiver<Stat>, all_chans: &AllChans) -> ! {
    info!("Starting monitoring task!");
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
    }
}
