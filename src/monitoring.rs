use crate::common::Stokes;
use crate::fpga::Device;
use crate::{capture::Stats, common::BLOCK_TIMEOUT};
use actix_web::{dev::Server, get, App, HttpResponse, HttpServer, Responder};
use lazy_static::lazy_static;
use prometheus::{
    register_gauge, register_gauge_vec, register_int_gauge, register_int_gauge_vec, Gauge,
    GaugeVec, IntGauge, IntGaugeVec, TextEncoder,
};
use thingbuf::mpsc::blocking::Receiver;
use thingbuf::mpsc::errors::RecvTimeoutError;
use tokio::sync::broadcast;
use tracing::{info, warn};

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
    static ref SHUFFLED_GAUGE: IntGauge = register_int_gauge!(
        "shuffled_packets",
        "Number of packets that were out of order"
    )
    .unwrap();
    static ref FFT_OVFL_GAUGE: IntGauge =
        register_int_gauge!("fft_ovfl", "Counter of FFT overflows").unwrap();
    static ref REQUANT_OVFL_GAUGE: IntGaugeVec = register_int_gauge_vec!(
        "requant_ovfl",
        "Counter of requantization overflows",
        &["polarization"]
    )
    .unwrap();
    static ref FPGA_TEMP: Gauge =
        register_gauge!("fpga_temp", "Internal FPGA temperature").unwrap();
    static ref ADC_RMS_GAUGE: GaugeVec =
        register_gauge_vec!("adc_rms", "RMS value of raw adc values", &["channel"]).unwrap();
}

#[get("/metrics")]
async fn metrics() -> impl Responder {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let body_str = encoder.encode_to_string(&metric_families).unwrap();
    HttpResponse::Ok().body(body_str)
}

pub fn monitor_task(
    device: Device,
    stats: Receiver<Stats>,
    avg: Receiver<Stokes>,
    mut shutdown: broadcast::Receiver<()>,
) -> eyre::Result<()> {
    info!("Starting monitoring task!");
    loop {
        // Look for shutdown signal
        if shutdown.try_recv().is_ok() {
            info!("Monitoring task stopping");
            break;
        }
        // Blocking here is ok, these are infrequent events
        match stats.recv_ref_timeout(BLOCK_TIMEOUT) {
            Ok(stat) => {
                PACKET_GAUGE.set(stat.processed.try_into().unwrap());
                DROP_GAUGE.set(stat.drops.try_into().unwrap());
                SHUFFLED_GAUGE.set(stat.shuffled.try_into().unwrap());
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Closed) => break,
            Err(_) => unreachable!(),
        }

        // Update channel data
        match avg.recv_ref_timeout(BLOCK_TIMEOUT) {
            Ok(avg_spec) => {
                for (i, v) in avg_spec.iter().enumerate() {
                    SPECTRUM_GAUGE
                        .with_label_values(&[&i.to_string()])
                        .set(f64::from(*v));
                }
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Closed) => break,
            Err(_) => unreachable!(),
        }

        // Metrics from the FPGA
        match device.fpga.fft_overflow_cnt.read() {
            Ok(v) => FFT_OVFL_GAUGE.set(u32::from(v).try_into().unwrap()),
            Err(e) => warn!("SNAP Error - {e}, {:?}", e),
        }

        match device.fpga.requant_a_overflow.read() {
            Ok(v) => REQUANT_OVFL_GAUGE
                .with_label_values(&["a"])
                .set(u32::from(v).try_into().unwrap()),
            Err(e) => warn!("SNAP Error - {e}, {:?}", e),
        }

        match device.fpga.requant_b_overflow.read() {
            Ok(v) => REQUANT_OVFL_GAUGE
                .with_label_values(&["b"])
                .set(u32::from(v).try_into().unwrap()),
            Err(e) => warn!("SNAP Error - {e}, {:?}", e),
        }

        match device.fpga.transport.lock().unwrap().temperature() {
            Ok(v) => FPGA_TEMP.set(v.try_into().unwrap()),
            Err(e) => warn!("SNAP Error - {e}, {:?}", e),
        }

        // Take a snapshot of ADC values and compute RMS value
        if device.fpga.adc_snap.arm().is_ok() && device.fpga.adc_snap.trigger().is_ok() {
            match device.fpga.adc_snap.read() {
                Ok(v) => {
                    let mut rms_a = 0.0;
                    let mut rms_b = 0.0;
                    let mut n = 0;
                    for chunk in v.chunks(4) {
                        rms_a += f64::powi(f64::from(chunk[0] as i8), 2);
                        rms_a += f64::powi(f64::from(chunk[1] as i8), 2);
                        rms_b += f64::powi(f64::from(chunk[2] as i8), 2);
                        rms_b += f64::powi(f64::from(chunk[3] as i8), 2);
                        n += 2;
                    }
                    rms_a = ((1.0 / (n as f64)) * rms_a).sqrt();
                    rms_b = ((1.0 / (n as f64)) * rms_b).sqrt();
                    ADC_RMS_GAUGE.with_label_values(&["a"]).set(rms_a);
                    ADC_RMS_GAUGE.with_label_values(&["b"]).set(rms_b);
                }
                Err(e) => warn!("SNAP Error - {e}, {:?}", e),
            }
        }
    }
    Ok(())
}

pub fn start_web_server(metrics_port: u16) -> eyre::Result<Server> {
    info!("Starting metrics webserver");
    let server = HttpServer::new(|| App::new().service(metrics))
        .bind(("0.0.0.0", metrics_port))?
        .run();
    Ok(server)
}
