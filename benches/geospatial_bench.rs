//! Benchmarks for the S2 geospatial module
//!
//! Measures the performance of:
//! - Bulk point insertion
//! - Exact-cell query
//! - Radius query
//! - Bounding-box query
//! - k-NN
//! - Aggregations
//! - Serialization / deserialization

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

use chrono::NaiveDateTime;
use tfg::config::MetricsConfig;
use tfg::geospatial::aggregation;
use tfg::geospatial::queries;
use tfg::geospatial::s2_index::S2Index;
use tfg::geospatial::serialization;
use tfg::ingestion::model::{DataQuality, GpsPoint, RoadType};
use tfg::metrics::delay_factor;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Generates N GPS points randomly distributed across Madrid
///
/// Uses a deterministic generator (fixed seed) for reproducibility
fn generate_madrid_points(n: usize) -> Vec<GpsPoint> {
    // Madrid bounding box
    let lat_min = 40.35;
    let lat_range = 0.15; // up to 40.50
    let lon_min = -3.80;
    let lon_range = 0.20; // up to -3.60

    let ts = NaiveDateTime::parse_from_str("2025-12-01 08:00:00", "%Y-%m-%d %H:%M:%S").unwrap();

    (0..n)
        .map(|i| {
            // Deterministic pseudo-uniform distribution
            let frac_lat = ((i * 7 + 13) % 1000) as f64 / 1000.0;
            let frac_lon = ((i * 11 + 29) % 1000) as f64 / 1000.0;

            GpsPoint {
                id: (i % 5000) as u32,
                timestamp: ts,
                latitude: lat_min + frac_lat * lat_range,
                longitude: lon_min + frac_lon * lon_range,
                road_type: if i % 3 == 0 {
                    RoadType::M30
                } else {
                    RoadType::Urban
                },
                intensidad: (100 + i % 900) as i32,
                ocupacion: (i % 100) as i32,
                carga: (i % 50) as i32,
                velocidad: if i % 3 == 0 { Some(60) } else { None },
                quality: DataQuality::Normal,
            }
        })
        .collect()
}

/// Builds an S2Index with N points to use in query benchmarks
fn build_index(n: usize, level: u64) -> S2Index {
    let points = generate_madrid_points(n);
    S2Index::build_from_iterator(points, level)
}

// ---------------------------------------------------------------------------
// Benchmarks
// ---------------------------------------------------------------------------

fn bench_insertion(c: &mut Criterion) {
    let mut group = c.benchmark_group("insertion");

    for size in [100, 1_000, 10_000].iter() {
        let points = generate_madrid_points(*size);

        group.bench_with_input(
            BenchmarkId::new("build_from_iterator", size),
            size,
            |b, _| {
                b.iter(|| {
                    let idx =
                        S2Index::build_from_iterator(black_box(points.clone()), black_box(15));
                    black_box(idx);
                });
            },
        );
    }

    group.finish();
}

fn bench_query_cell(c: &mut Criterion) {
    let index = build_index(10_000, 15);
    // Take a real cell_id from the index
    let cell_id = index.cell_ids()[0];

    c.bench_function("query_cell_10k", |b| {
        b.iter(|| {
            let result = queries::query_cell(black_box(&index), black_box(&cell_id));
            black_box(result);
        });
    });
}

fn bench_query_radius(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_radius");
    let index = build_index(10_000, 15);

    for radius in [0.5, 1.0, 5.0].iter() {
        group.bench_with_input(
            BenchmarkId::new("10k_pts", format!("{radius}km")),
            radius,
            |b, r| {
                b.iter(|| {
                    let result = queries::query_radius(
                        black_box(&index),
                        black_box(40.42),
                        black_box(-3.70),
                        black_box(*r),
                    );
                    black_box(result);
                });
            },
        );
    }

    group.finish();
}

fn bench_query_rect(c: &mut Criterion) {
    let index = build_index(10_000, 15);

    c.bench_function("query_rect_10k", |b| {
        b.iter(|| {
            let result = queries::query_rect(
                black_box(&index),
                black_box(40.40),
                black_box(40.45),
                black_box(-3.75),
                black_box(-3.68),
            );
            black_box(result);
        });
    });
}

fn bench_query_knn(c: &mut Criterion) {
    let mut group = c.benchmark_group("query_knn");
    let index = build_index(10_000, 15);

    for k in [5, 20, 100].iter() {
        group.bench_with_input(BenchmarkId::new("10k_pts", k), k, |b, k| {
            b.iter(|| {
                let result = queries::query_knn(
                    black_box(&index),
                    black_box(40.42),
                    black_box(-3.70),
                    black_box(*k),
                );
                black_box(result);
            });
        });
    }

    group.finish();
}

fn bench_aggregation(c: &mut Criterion) {
    let mut group = c.benchmark_group("aggregation");
    let index = build_index(10_000, 17);

    group.bench_function("aggregate_all_10k", |b| {
        b.iter(|| {
            let result = aggregation::aggregate_all(black_box(&index));
            black_box(result);
        });
    });

    group.bench_function("aggregate_by_level15_10k", |b| {
        b.iter(|| {
            let result = aggregation::aggregate_by_level(black_box(&index), black_box(15));
            black_box(result);
        });
    });

    group.finish();
}

fn bench_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("serialization");
    let index = build_index(10_000, 15);
    let path = "/tmp/bench_s2_index.bin";

    group.bench_function("save_10k", |b| {
        b.iter(|| {
            serialization::save_index(black_box(&index), black_box(path)).unwrap();
        });
    });

    // Ensure file exists for load benchmark
    serialization::save_index(&index, path).unwrap();

    group.bench_function("load_10k", |b| {
        b.iter(|| {
            let loaded = serialization::load_index(black_box(path)).unwrap();
            black_box(loaded);
        });
    });

    // Cleanup
    std::fs::remove_file(path).ok();
    group.finish();
}

fn bench_delay_factor(c: &mut Criterion) {
    let mut group = c.benchmark_group("metrics_delay_factor");
    let index = build_index(50_000, 15);
    let config = MetricsConfig {
        m30_reference_speed: 90.0,
        urban_reference_speed: 50.0,
        min_observations: 5,
    };

    group.bench_function("compute_all_cells_50k", |b| {
        b.iter(|| {
            let result = delay_factor::compute_all_cells(black_box(&index), black_box(&config));
            black_box(result);
        });
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Criterion harness
// ---------------------------------------------------------------------------

criterion_group!(
    benches,
    bench_insertion,
    bench_query_cell,
    bench_query_radius,
    bench_query_rect,
    bench_query_knn,
    bench_aggregation,
    bench_serialization,
    bench_delay_factor,
);
criterion_main!(benches);
