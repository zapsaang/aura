use aura_daemon::collectors;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_collect_sample(c: &mut Criterion) {
    let mut state = collectors::CollectorState::new();
    collectors::init(&mut state).expect("collector init");

    c.bench_function("collectors::collect_sample", |b| {
        b.iter(|| {
            let sample = collectors::collect_sample(black_box(&mut state)).expect("collect_sample");
            black_box(sample.archive.meta.timestamp_ns);
        })
    });
}

criterion_group!(collector_benches, bench_collect_sample);
criterion_main!(collector_benches);
