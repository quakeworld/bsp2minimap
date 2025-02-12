use std::fs;

use criterion::{criterion_group, criterion_main, Criterion};

fn lib_benchmark(c: &mut Criterion) {
    // group.throughput(Throughput::Bytes(data.len() as u64));

    let file = &mut fs::File::open("tests/files/dm3_gpl.bsp").unwrap();
    let bsp = bspparser::BspFile::parse(file).unwrap();

    let mut group = c.benchmark_group("lib");
    group.bench_function("convert", |b| {
        b.iter(|| bsp2svg::filter_and_sort_faces(&bsp, &bsp2svg::ProjectionAxis::Z))
    });
    group.finish();
}

criterion_group!(benches, lib_benchmark);
criterion_main!(benches);
