use crate::capture::Stats;
use crate::common::{AllChans, Stokes};
use crate::fpga::Device;
use crossbeam::channel::Receiver;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::header::CONTENT_TYPE;
use hyper::http::HeaderValue;
use hyper::{Request, Response};
use lazy_static::lazy_static;
use log::{info, warn};
use prometheus::{
    linear_buckets, register_gauge, register_gauge_vec, register_histogram_vec, register_int_gauge,
    register_int_gauge_vec, Encoder, Gauge, GaugeVec, HistogramVec, IntGauge, IntGaugeVec,
    TextEncoder,
};
use std::convert::Infallible;

lazy_static! {
    static ref CHANNEL_GAUGE: IntGaugeVec = register_int_gauge_vec!(
        "task_channel_backlog",
        "Number of yet-to-be-processed data in each inter-task channel",
        &["target_channel"]
    )
    .unwrap();
    static ref SPECTRUM_GAUGE: GaugeVec =
        register_gauge_vec!("spectrum", "Average spectrum data", &["channel"]).unwrap();
    static ref PACKET_GAUGE: IntGauge =
        register_int_gauge!("processed_packets", "Number of packets we've processed").unwrap();
    static ref DROP_GAUGE: IntGauge =
        register_int_gauge!("dropped_packets", "Number of packets we've dropped").unwrap();
    static ref FFT_OVFL_GAUGE: IntGauge =
        register_int_gauge!("fft_ovfl", "Counter of FFT overflows").unwrap();
    static ref REQUANT_OVFL_GAUGE: IntGauge =
        register_int_gauge!("requant_ovfl", "Counter of requantization overflows").unwrap();
    static ref FPGA_TEMP: Gauge =
        register_gauge!("fpga_temp", "Internal FPGA temperature").unwrap();
    static ref RAW_ADC_HIST: HistogramVec = register_histogram_vec!(
        "raw_adc_hist",
        "Histogram data for raw ADC counts",
        &["polarization"],
        linear_buckets(-127.0, 4.0, 256).unwrap()
    )
    .unwrap();
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
#[allow(clippy::cast_possible_wrap)]
#[allow(clippy::similar_names)]
#[allow(clippy::missing_panics_doc)]
pub fn monitor_task(
    all_chans: &AllChans,
    device: &Device,
    spec_rcv: &Receiver<Stokes>,
    cap_stat_rcv: &Receiver<Stats>,
) -> ! {
    info!("Starting monitoring task!");
    loop {
        // Blocking here is ok, these are infrequent events
        let stat = cap_stat_rcv.recv().unwrap();

        // Then wait for spectrum
        let avg_spec = spec_rcv.recv().unwrap();

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

        // Take a snapshot of ADC values and add to histogram
        if device.fpga.adc_snap.arm().is_ok() && device.fpga.adc_snap.trigger().is_ok() {
            if let Ok(v) = device.fpga.adc_snap.read() {
                for chunk in v.chunks(4) {
                    RAW_ADC_HIST
                        .with_label_values(&["a"])
                        .observe(f64::from(chunk[0] as i8));
                    RAW_ADC_HIST
                        .with_label_values(&["a"])
                        .observe(f64::from(chunk[1] as i8));
                    RAW_ADC_HIST
                        .with_label_values(&["b"])
                        .observe(f64::from(chunk[2] as i8));
                    RAW_ADC_HIST
                        .with_label_values(&["b"])
                        .observe(f64::from(chunk[3] as i8));
                }
            } else {
                warn!("Error reading ADC snapshot");
            }
        }

        // Update metrics
        PACKET_GAUGE.add(stat.captured.try_into().unwrap());
        DROP_GAUGE.add(stat.dropped.try_into().unwrap());
        CHANNEL_GAUGE
            .with_label_values(&["to_downsample"])
            .set(all_chans.to_downsample.len().try_into().unwrap());
        CHANNEL_GAUGE
            .with_label_values(&["to_dump"])
            .set(all_chans.to_dump.len().try_into().unwrap());
        CHANNEL_GAUGE
            .with_label_values(&["to_exfil"])
            .set(all_chans.to_exfil.len().try_into().unwrap());
        CHANNEL_GAUGE
            .with_label_values(&["to_sort"])
            .set(all_chans.to_sort.len().try_into().unwrap());

        // Update channel data
        for (i, v) in avg_spec.into_iter().enumerate() {
            SPECTRUM_GAUGE
                .with_label_values(&[&i.to_string()])
                .set(f64::from(v));
        }
    }
}
