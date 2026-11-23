//! Urban Mobility Analysis Engine — Madrid
//!
//! Binary entry point. Supports two modes of use:
//!
//! ## TUI mode (no arguments)
//!
//! ```bash
//! ./tfg
//! ```
//!
//! Launches the interactive TUI with a selection menu and a live panel
//!
//! ## CLI mode (with arguments)
//!
//! ```bash
//! ./tfg demo
//! ./tfg live --ticks 3
//! ./tfg export --format geojson
//! ```

use anyhow::{Context, Result};
use clap::Parser;

use tfg::cli::commands;
use tfg::cli::opts::{Cli, Command};
use tfg::config::Config;

fn main() -> Result<()> {
    // With no arguments, start the TUI
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 1 {
        return tfg::tui::run_tui();
    }

    // CLI mode: arguments present -> parse them (--help is handled automatically)
    let cli = Cli::parse();

    let filter = format!("tfg={}", cli.log_level);
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .without_time()
        .init();

    let config = Config::from_file(&cli.config)
        .with_context(|| format!("Could not load '{}'", cli.config.display()))?;

    match &cli.command {
        Command::Demo(args) => commands::run_demo(args, &config)?,
        Command::Live(args) => commands::run_live(args, &config)?,
        Command::Export(args) => commands::run_export(args, &config)?,
    }

    Ok(())
}
