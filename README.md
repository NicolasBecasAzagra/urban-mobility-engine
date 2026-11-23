# Urban Mobility Engine — Madrid

A native Rust engine from a BSc thesis in Computer Engineering. It indexes GPS points with S2 cells (a `HashMap<CellID, Vec<GpsPoint>>`, base level 15 with adaptive subdivision up to level 17), computes per-cell delay statistics in parallel with rayon, and flags congestion anomalies against a historical baseline. It polls Madrid's live traffic API every 5 minutes.

[![Rust](https://img.shields.io/badge/rust-1.87+-orange.svg)](https://www.rust-lang.org)
[![Tests](https://img.shields.io/badge/tests-240%20passing-green.svg)](#tests)
[![Version](https://img.shields.io/badge/version-1.0.0-blue.svg)](CHANGELOG.md)

Native **Rust** engine for near-real-time urban traffic analysis.  
It combines historical data with the [Madrid City Council traffic API](https://informo.madrid.es) to detect anomalies and congested zones in real time.

📄 The full thesis write-up (in Spanish) is available [here](https://drive.google.com/file/d/1mGmO571RynBTtX9S1QJqbsnNb1kY3vA_/view?usp=sharing).

---

## Quick start

```bash
# Clone and build
git clone https://github.com/NicolasBecasAzagra/urban-mobility-engine.git
cd urban-mobility-engine
cargo build --release

# Interactive TUI (recommended mode — no arguments)
./target/release/tfg

# Direct CLI — historical pipeline
./target/release/tfg demo

# Direct CLI — live mode against the Madrid API
./target/release/tfg live --ticks 3

# Direct CLI — preloading a historical baseline
./target/release/tfg live --warm-start data/samples/hora_punta.csv --ticks 5

# Export in different formats
./target/release/tfg export --format json
./target/release/tfg export --format csv
./target/release/tfg export --format geojson
```

---

## Usage modes

### TUI mode (interactive)

```
./target/release/tfg
```

Launches a fully interactive terminal interface with two phases:

**Phase 1 — Start menu:**
```
┌──────────────────────────────────────────────────────┐
│  Urban Mobility Engine — Madrid   v1.0.0             │
├──────────────────────────────────────────────────────┤
│   Select the analysis mode:                          │
│                                                      │
│   ❯ Start real-time analysis                         │
│     Connects to the Madrid City Council API          │
│                                                      │
│     Export latest results                            │
│     View performance metrics                         │
│     Quit                                             │
│                                                      │
│   [↑↓] Navigate   [Enter] Select   [q] Quit          │
└──────────────────────────────────────────────────────┘
```

**Phase 2 — Live panel:**
```
┌─── Engine Status ────────────────────┐ ┌─── Active Alerts ────────────────────┐
│  Tick: 3    Source: Madrid API        │ │  3 anomalies detected                │
│  Indexed points: 18,543               │ │                                      │
│  Active S2 cells: 1,204               │ │  [SEVERE]   SRF 0.28 (ref 0.71)      │
│  Mean SRF: 0.71   Congested: 47       │ │  [MODERATE] SRF 0.41 (ref 0.68)      │
│  Active alerts: 3                     │ │  [IMPROVEMENT] SRF 0.95 (ref 0.45)   │
│                                       │ │                                      │
│  Next tick in: 298s ████░░░░          │ │  Tick History                        │
│                                       │ │  Tick #1: 350 pts | SRF 0.70 | ok    │
│  [q] Quit  [p] Pause  [e] Export      │ │  Tick #2: 380 pts | SRF 0.65 | ok    │
└───────────────────────────────────────┘ └──────────────────────────────────────┘
```

### CLI mode (direct)

```bash
./target/release/tfg live --ticks 3          # 3 ticks from the live API
./target/release/tfg demo                    # Full historical pipeline
./target/release/tfg export --format geojson # Export to GeoJSON
```

---

## Architecture

```
tfg/
├── src/
│   ├── ingestion/          # Data ingestion
│   │   ├── csv_parser.rs   # Streaming CSV parser with a lazy Iterator
│   │   ├── csv_source.rs   # CsvDataSource: implements DataSource
│   │   ├── datasource.rs   # DataSource trait (unified abstraction)
│   │   ├── live_api.rs     # LiveApiSource: Madrid API XML parser
│   │   └── scheduler.rs    # PollingScheduler: tick cycles + anomalies
│   ├── geospatial/         # S2 indexing
│   │   ├── s2_index.rs     # S2Index: HashMap<CellID, Vec<GpsPoint>>
│   │   └── serialization.rs# Binary save/load of the index (bincode)
│   ├── metrics/            # Metrics engine
│   │   ├── model.rs        # DelayFactor, CellDelayReport, CongestionLevel
│   │   ├── delay_factor.rs # Per-cell SRF computation (parallel with rayon)
│   │   ├── statistics.rs   # Per-cell IQR percentiles
│   │   ├── anomaly.rs      # AnomalyDetector: live SRF vs baseline
│   │   └── temporal.rs     # Temporal aggregations (TimeSlot, DayType)
│   ├── export/             # Multi-format export
│   │   ├── json_export.rs  # JSON with tick_alerts for the dashboard
│   │   ├── csv_export.rs   # Flat CSV for external analysis
│   │   └── geojson_export.rs# GeoJSON for QGIS / Leaflet
│   ├── tui/                # Interactive TUI (ratatui)
│   │   ├── app.rs          # App: global TUI state
│   │   ├── events.rs       # handle_key_event: keyboard controls
│   │   ├── menu.rs         # render_menu: start screen
│   │   └── live_panel.rs   # render_live_panel: real-time panel
│   ├── cli/                # CLI (clap)
│   │   ├── commands.rs     # run_demo / run_live / run_export
│   │   └── opts.rs         # Cli struct with subcommands
│   ├── config.rs           # Config: reads config.toml
│   └── progress.rs         # Progress bars (indicatif)
```

### Data flow

```
Historical CSV (warm-start)
       │
       ▼
GpsPoint → S2Index (HashMap<CellID, Vec<GpsPoint>>)
       │
       ▼
compute_all_statistics() → CellStatistics (p25, p50, p75, p95, IQR)
       │
       ▼
AnomalyDetector::from_statistics() → per-cell baseline
       │
       │        Madrid API (every 5 min)
       │               │
       ▼               ▼
PollingScheduler::tick() → CellDelayReport + AnomalyAlert
       │
       ▼
TUI (ratatui) + JSON export (dashboard.html)
```

---

## Performance results

| Metric | Value |
|---------|-------|
| SRF computation (50,000 points, rayon, Apple M1) | **~312 µs** (Criterion) |
| Share of the 5-minute polling interval | **< 0.001%** |
| Total tests | **240 passing** (1 live-API test ignored) |
| Clippy warnings (`-D warnings`) | **0** |

Benchmarks can be run with:

```bash
cargo bench
# Generates an HTML report in target/criterion/geospatial_bench/
```

---

## Tests

```bash
# All tests
cargo test

# Tests for a specific module
cargo test metrics::anomaly
cargo test tui::
cargo test --test test_export

# With full output
cargo test -- --nocapture
```

---

## Configuration

The `config.toml` file controls all engine parameters:

```toml
[geospatial]
base_level = 15          # Base S2 grid level (15 ≈ 1-2 km²)
subdivision_level = 17   # Maximum level after adaptive subdivision
delay_threshold = 1.5    # SRF threshold for subdividing cells
min_gps_count = 50       # Minimum points needed to compute SRF

[metrics]
m30_reference_speed = 90.0     # M-30 reference speed (km/h)
urban_reference_speed = 50.0   # Urban reference speed (km/h)
min_observations = 1           # Minimum observations per cell

[live]
endpoint = "https://informo.madrid.es/informo/tmadrid/pm.xml"
locations_path = "data/samples/locations.csv"
poll_interval_secs = 300       # 5 minutes (API cycle)
```

---

## Key dependencies

| Crate | Version | Use |
|-------|---------|-----|
| `s2` | 0.0.12 | S2 geospatial indexing |
| `rayon` | 1.12 | Parallelism in the metrics engine |
| `ratatui` | 0.28 | Interactive TUI |
| `crossterm` | 0.27 | Cross-platform terminal backend |
| `clap` | 4 | Declarative CLI |
| `serde_json` | 1 | JSON export |
| `quick-xml` | 0.37 | XML parser for the Madrid API |
| `indicatif` | 0.17 | Progress bars |
| `tracing` | 0.1 | Structured logging |

---

## License

BSc thesis project — Urban Mobility Engine — Madrid  
BSc in Computer Engineering — 2025/2026
