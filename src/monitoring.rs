use crate::common::AllChans;
use crossbeam::channel::Receiver;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::header::CONTENT_TYPE;
use hyper::http::HeaderValue;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response};
use lazy_static::lazy_static;
use log::info;
use pcap::Stat;
use prometheus::{register_gauge, register_gauge_vec, Encoder, Gauge, GaugeVec, TextEncoder};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::Instant;
use tokio::net::TcpListener;

lazy_static! {
    static ref CHANNEL_GAUGE: GaugeVec = register_gauge_vec!(
        "task_channel_backlog",
        "Number of yet-to-be-processed data in each inter-task channel",
        &["to_split", "to_downsample", "to_dump", "to_exfil"]
    )
    .unwrap();
    static ref PPS_GAUGE: Gauge =
        register_gauge!("pps", "Number of packets we're processing per second").unwrap();
    static ref DPS_GAUGE: Gauge =
        register_gauge!("pps", "Number of packets we're processing per second").unwrap();
}

#[allow(clippy::missing_panics_doc)]
async fn metrics(_: Request<hyper::body::Incoming>) -> Result<Response<Full<Bytes>>, Infallible> {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let encoded = encoder.encode_to_string(&metric_families).unwrap();
    let mut resp = Response::new(Full::new(encoded.into()));
    resp.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_str(encoder.format_type()).unwrap(),
    );
    Ok(resp)
}

#[allow(clippy::cast_precision_loss)]
#[allow(clippy::similar_names)]
pub async fn monitor_task(
    stat_receiver: &Receiver<Stat>,
    all_chans: &AllChans,
    metrics_port: u16,
) -> ! {
    info!("Starting monitoring task!");
    let mut last_state = Instant::now();
    let mut last_rcv = 0;
    let mut last_drops = 0;
    let mut header = false;

    let addr = ([0, 0, 0, 0], metrics_port).into();
    let listener = TcpListener::bind(addr).await.unwrap();

    loop {
        let (stream, _) = listener.accept().await?;
        // Blocking here is ok, these are infrequent events
        let stat = stat_receiver.recv().expect("Stat receive channel error");
        let since_last = last_state.elapsed();
        last_state = Instant::now();
        let pps = (stat.received - last_rcv) as f32 / since_last.as_secs_f32();
        let dps = (stat.dropped - last_drops) as f32 / since_last.as_secs_f32();
        last_rcv = stat.received;
        last_drops = stat.dropped;
    }
}
