# Changelog — Urban Mobility Engine — Madrid

All notable changes to this project are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.0.0/)
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [1.0.0] — 2026-05-11

### Sprint 6 (Tasks 29–31) — Anomaly Detection · TUI · Release

#### Added

- **`metrics::anomaly` module**: detection of live SRF anomalies against the historical baseline
  - `AnomalyDetector::from_statistics()` — builds the baseline (p5/p25/p50/p75/p95) from historical IQR data
  - `AnomalyDetector::detect()` — classifies alerts as Severe / Moderate / Improvement
  - `AnomalyAlert` with a human-readable `description()` for the TUI and logs
  - 15 unit tests covering every scenario in the spec
- **`TickReport::anomaly_alerts`**: the tick's alerts are included in every scheduler cycle
- **`PollingScheduler::with_anomaly_detector()`**: builder to attach the detector
- **`run_live()` with AnomalyDetector**: builds the baseline during warm-start and prints alerts to the console
- **JSON export with `tick_alerts`**: the field appears in `live_output.json` only when there are alerts
- **`tui` module** with ratatui 0.28 + crossterm 0.27:
  - `App` — global state with tick history and countdown
  - `events::handle_key_event` — keyboard controls (↑↓, Enter, q, p, e)
  - `menu::render_menu` — start screen with 4 options and descriptions
  - `live_panel::render_live_panel` — two-column panel (status + alerts + history)
  - `render_countdown_bar` — `Gauge` countdown bar to the next tick
  - 19 unit tests of the TUI logic that do not depend on the terminal
- **Dual binary mode**: `./tfg` → TUI, `./tfg <subcommand>` → direct CLI

#### Changed

- `Cargo.toml`: version `1.0.0`, new `ratatui` and `crossterm` dependencies
- `src/main.rs`: `try_parse()` instead of `parse()` to detect missing arguments
- `src/lib.rs`: exposed the `tui` module
- `README.md`: rewritten with the full architecture, usage modes and results

#### Fixed

- 0 warnings from `cargo clippy -- -D warnings`
- Unused imports in the `tui` modules
- Doc-comment indentation in `anomaly.rs`
- Wildcard pattern in the TUI menu `match`

---

## [0.5.0] — 2026-05-03

### Sprint 5 (Tasks 25–28) — Export · CLI · Logging · Rayon

#### Added

- **`export` module**: export to JSON, CSV and GeoJSON
  - `export_json()` — JSON compatible with `dashboard_live.html`
  - `export_csv()` — flat CSV for analysis in Python/R/Excel
  - `export_geojson()` — standard GeoJSON for QGIS / Leaflet / Mapbox
- **CLI with `clap`**: `demo`, `live` and `export` subcommands
  - `--warm-start` to preload a CSV before live polling
  - `--ticks` to limit the number of cycles
  - `--format json|csv|geojson` to choose the export format
  - `--log-level` to control the tracing level
- **`progress` module**: progress bars and spinners with `indicatif`
  - `phase_header()`, `phase_done()`, `pipeline_done()`
  - `tick_summary()` with a visual summary of each live tick
- **Parallelism with `rayon`**:
  - `compute_all_cells()` parallelized with `par_iter()`
  - Performance: ~888 µs for 50,000 points on 8 cores
  - 0.03% of the time available between API ticks
- Web dashboard `demo/dashboard_live.html` with auto-refresh

#### Changed

- `PollingScheduler::tick()` with adaptive subdivision of congested cells

---

## [0.4.0] — 2026-04-21

### Sprint 4 (Tasks 23–24) — PollingScheduler · Live Tests

#### Added

- **`PollingScheduler`**: periodic fetch → index → compute cycles
  - Incremental `S2Index` updates (no rebuild)
  - `run(max_ticks)` with a sleep between ticks based on `refresh_interval()`
  - `with_anomaly_detector()` (prepared for Sprint 6)
- **Warm-start mode**: preloads historical CSV before live polling
- **Integration tests** for live mode (`tests/test_live_pipeline.rs`):
  - 9 tests with a real XML fixture from the Madrid API
  - `test_scheduler_with_xml_fixture_single_tick()`
  - `test_full_pipeline_via_scheduler()`

---

## [0.3.0] — 2026-04-08

### Sprint 3 (Tasks 21–22) — DataSource Trait · LiveApiSource

#### Added

- **`DataSource` trait**: unified abstraction for data sources
  - `fetch_batch()` — fetch a batch of GPS points
  - `has_more()` — indicates whether more data remains
  - `refresh_interval()` — polling interval
- **`CsvDataSource`**: implements DataSource for CSV files
- **`LiveApiSource`**: XML parser for the Madrid City Council traffic API
  - Handling of the UTF-8 BOM in the HTTP response
  - Inconsistent timestamps (multiple formats)
  - Silent fallback for sensors without GPS coordinates

---

## [0.2.0] — Sprint 2 (Tasks 15–20)

### SRF Engine · Statistics · End-to-end Pipeline

#### Added

- **SRF engine (delay_factor)**: Speed Reduction Factor adapted from the HCM
  - Urban roads: `SRF = 1.0 - load/100`
  - M-30: `SRF = observed_speed / reference_speed`
  - Congestion levels A–F inspired by the Highway Capacity Manual
- **Per-cell statistics** (`CellStatistics`):
  - Percentiles p25, p50, p75, p95 with NIST linear interpolation
  - IQR and anomaly count using Tukey's method
- **Temporal aggregations** (`temporal.rs`):
  - `TimeSlot`: 6 time slots (early morning, morning peak, midday...)
  - `DayType`: Weekday / Weekend
- **End-to-end pipeline**: CSV → S2 → metrics → statistics → JSON
- **Integration tests** for the metrics engine (`tests/test_metrics.rs`)
- **Validation against** reference-speed baselines

---

## [0.1.0] — Sprint 1 (Tasks 1–14)

### Foundations — CSV Ingestion · S2Index · Serialization · Benchmarks

#### Added

- **Streaming CSV parser** with a lazy `Iterator`
  - Reading of `hora_punta.csv` and `locations.csv`
  - Header mapping and `GpsPoint` construction
  - Error handling with `thiserror` and `anyhow`
- **`S2Index`** (`HashMap<CellID, Vec<GpsPoint>>`):
  - Single and bulk insertion of GPS points
  - Queries: by cell, by radius, by rectangle, spatial k-NN
  - `S2RegionCoverer` for polygon coverage
  - Adaptive subdivision of congested cells
  - Spatial aggregations by cell level
- **Binary serialization** of the S2 index with `bincode` (`save`/`load`)
- **Benchmarks** with `criterion`:
  - `geospatial_bench`: insertion, radius search, k-NN
  - HTML reports in `target/criterion/`
- **Custom error system** with `thiserror`
- **External configuration** with `config.toml` and `AppConfig`
- **CI/CD pipeline** with GitHub Actions
