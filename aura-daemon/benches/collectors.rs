use aura_daemon::collectors;
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_collect_all(c: &mut Criterion) {
    let mut state = collectors::CollectorState::new();
    collectors::init(&mut state).expect("collector init");

    c.bench_function("collectors::collect_all", |b| {
        b.iter(|| {
            collectors::collect_all(black_box(&mut state)).expect("collect_all");
            black_box(state.telemetry.meta.timestamp_ns);
        })
    });
}

criterion_group!(collector_benches, bench_collect_all);
criterion_main!(collector_benches);
