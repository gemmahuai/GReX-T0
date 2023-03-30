use criterion::{black_box, criterion_group, criterion_main, Criterion};
use grex_t0::{common::Payload, dumps::DumpRing};

pub fn push_ring(c: &mut Criterion) {
    let mut dr = DumpRing::new(15);
    let pl = Payload::default();
    c.bench_function("push ring", |b| {
        b.iter(|| {
            dr.next_push().clone_from(black_box(&pl));
        })
    });
}

pub fn to_ndarray(c: &mut Criterion) {
    let payload = Payload::default();
    c.bench_function("payload to nd", |b| {
        b.iter(|| black_box(payload.into_ndarray()))
    });
}

criterion_group!(benches, push_ring, to_ndarray,);
criterion_main!(benches);
