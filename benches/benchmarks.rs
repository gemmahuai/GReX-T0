use std::time::Instant;

use criterion::{black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use grex_t0::{
    capture::PAYLOAD_SIZE,
    common::{stokes_i, Channel, Payload, CHANNELS},
    dumps::DumpRing,
};
use rand::prelude::*;

pub fn stokes(c: &mut Criterion) {
    let pola = [Channel::default(); CHANNELS];
    let polb = [Channel::default(); CHANNELS];
    c.bench_function("stokes i", |b| {
        b.iter(|| stokes_i(black_box(&pola), black_box(&polb)))
    });
}

fn decode(c: &mut Criterion) {
    let mut rng = rand::thread_rng();
    c.bench_function("payload unpack", |b| {
        b.iter_batched(
            || {
                // Setup by creating random bytes
                let mut bytes = [0u8; PAYLOAD_SIZE];
                rng.fill(&mut bytes[..]);
                bytes
            },
            |bytes| {
                // Execute
                Payload::from_bytes(black_box(&bytes))
            },
            BatchSize::SmallInput,
        )
    });
}

fn downsample_stokes(c: &mut Criterion) {
    let mut rng = rand::thread_rng();
    let mut group = c.benchmark_group("downsample_stokes");
    for downsample_factor in [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024].iter() {
        group.bench_with_input(
            BenchmarkId::from_parameter(downsample_factor),
            downsample_factor,
            |b, &downsample_factor| {
                b.iter_custom(|iters| {
                    // Create payloads
                    let mut payloads = vec![];
                    for _i in 0..iters {
                        let mut bytes = [0u8; PAYLOAD_SIZE];
                        rng.fill(&mut bytes[..]);
                        payloads.push(Payload::from_bytes(&bytes));
                    }

                    // Setup state
                    let mut avg = [0f32; CHANNELS];
                    let mut idx = 0usize;

                    let start = Instant::now();
                    for i in 0..iters {
                        avg.iter_mut()
                            .zip(payloads[i as usize].stokes_i())
                            .for_each(|(x, y)| *x += y);
                        // If we're at the end, calculate the average
                        if idx == downsample_factor - 1 {
                            // Find the average into an f32 (which is lossless)
                            avg.iter_mut()
                                .for_each(|v| *v /= f32::from(downsample_factor as u16));
                            // And zero the state
                            avg = [0f32; CHANNELS];
                        }
                        // Increment the idx
                        idx = (idx + 1) % downsample_factor;
                    }
                    start.elapsed()
                })
            },
        );
    }
    group.finish();
}

pub fn push_ring(c: &mut Criterion) {
    let mut dr = DumpRing::new(1_048_576);
    c.bench_function("push ring", |b| {
        b.iter_batched(
            Payload::default,
            |pl| {
                dr.push(black_box(pl));
            },
            BatchSize::SmallInput,
        )
    });
}

pub fn to_ndarray(c: &mut Criterion) {
    let payload = Payload::default();
    c.bench_function("payload to nd", |b| {
        b.iter(|| black_box(payload.into_ndarray()))
    });
}

criterion_group!(
    benches,
    stokes,
    push_ring,
    to_ndarray,
    downsample_stokes,
    decode
);
criterion_main!(benches);
