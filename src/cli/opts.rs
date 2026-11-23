//! CLI command interface definition (clap)
//!
//! Subcommand structure:
//!
//! ```text
//! tfg [--config <path>] [--log-level <level>] <subcommand>
//!
//! Subcommands:
//!   demo    Historical pipeline: CSV → S2 → metrics → dashboard
//!   live    Near-real-time mode: polling of the Madrid API
//!   export  Export results to JSON, CSV or GeoJSON
//! ```

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

// ---------------------------------------------------------------------------
// Root CLI
// ---------------------------------------------------------------------------

/// Urban Mobility Analysis Engine — Madrid
///
/// Processes real-time and historical traffic data using
/// S2 geospatial indexing and flow-speed metrics (SRF)
#[derive(Parser, Debug)]
#[command(
    name = "tfg",
    version = "1.0.0",
    author = "Nicolas Becas",
    about = "Urban Mobility Analysis Engine — Madrid",
    long_about = "\
Urban mobility analysis engine based on S2 geospatial indexing.\n\
Computes the Speed Reduction Factor (SRF) per cell using data from the\n\
Madrid City Council and exports it to JSON/CSV/GeoJSON.\n\
\n\
EXAMPLES:\n\
  tfg demo\n\
  tfg live --ticks 3\n\
  tfg live --warm-start data/samples/hora_punta.csv --ticks 5\n\
  tfg export --format geojson --output demo/madrid.geojson"
)]
pub struct Cli {
    /// Path to the TOML configuration file
    #[arg(long, default_value = "config.toml", global = true)]
    pub config: PathBuf,

    /// Log verbosity level (error/warn/info/debug/trace)
    #[arg(long, default_value = "info", global = true)]
    pub log_level: String,

    #[command(subcommand)]
    pub command: Command,
}

// ---------------------------------------------------------------------------
// Subcommands
// ---------------------------------------------------------------------------

/// Available CLI subcommands
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Complete historical pipeline: CSV → S2 → metrics → dashboard
    ///
    /// Loads historical traffic data from CSV, indexes it on the
    /// S2 grid, computes congestion metrics and exports a
    /// JSON file ready to visualize in dashboard.html
    Demo(DemoArgs),

    /// Near-real-time mode: polls the Madrid API every 5 minutes
    ///
    /// Connects to the Madrid City Council traffic API
    /// and updates the S2 index on every tick. Exports the current
    /// state after each tick so dashboard.html refreshes
    Live(LiveArgs),

    /// Exports pipeline results to JSON, CSV or GeoJSON
    ///
    /// Runs the historical pipeline and exports the result in the
    /// chosen format. The JSON is compatible with dashboard.html,
    /// the CSV with Excel/Python/R, and the GeoJSON with QGIS/Leaflet
    Export(ExportArgs),
}

// ---------------------------------------------------------------------------
// DemoArgs
// ---------------------------------------------------------------------------

/// Arguments of the `tfg demo` subcommand
#[derive(Args, Debug)]
pub struct DemoArgs {
    /// CSV file with historical traffic data
    #[arg(long, default_value = "data/samples/hora_punta.csv")]
    pub data: PathBuf,

    /// CSV file with sensor locations
    #[arg(long, default_value = "data/samples/locations.csv")]
    pub locations: PathBuf,

    /// Output directory for the generated files
    #[arg(long, default_value = "demo/")]
    pub output: PathBuf,
}

// ---------------------------------------------------------------------------
// LiveArgs
// ---------------------------------------------------------------------------

/// Arguments of the `tfg live` subcommand
#[derive(Args, Debug)]
pub struct LiveArgs {
    /// Historical CSV used to initialize the index before polling (warm-start)
    ///
    /// If specified, the S2 index is preloaded with historical data
    /// before API polling starts. Useful to have a
    /// reference baseline from the very first tick
    #[arg(long)]
    pub warm_start: Option<PathBuf>,

    /// CSV file with sensor locations (for warm-start)
    #[arg(long, default_value = "data/samples/locations.csv")]
    pub locations: PathBuf,

    /// Number of ticks to run (0 = continuous until Ctrl+C)
    #[arg(long, default_value = "0")]
    pub ticks: usize,

    /// Output JSON file (compatible with dashboard.html)
    #[arg(long, default_value = "demo/live_output.json")]
    pub output: PathBuf,
}

// ---------------------------------------------------------------------------
// ExportArgs
// ---------------------------------------------------------------------------

/// Available export formats
#[derive(ValueEnum, Clone, Debug)]
pub enum ExportFormat {
    /// Structured JSON compatible with dashboard.html
    Json,
    /// Flat CSV for Excel / Python / R
    Csv,
    /// Standard GeoJSON for QGIS / Leaflet / Mapbox
    Geojson,
}

/// Arguments of the `tfg export` subcommand
#[derive(Args, Debug)]
pub struct ExportArgs {
    /// Export format
    #[arg(long, value_enum, default_value = "json")]
    pub format: ExportFormat,

    /// Output directory (or exact file if an extension is given)
    #[arg(long, default_value = "demo/")]
    pub output: PathBuf,

    /// CSV file with historical traffic data
    #[arg(long, default_value = "data/samples/hora_punta.csv")]
    pub data: PathBuf,

    /// CSV file with sensor locations
    #[arg(long, default_value = "data/samples/locations.csv")]
    pub locations: PathBuf,
}
