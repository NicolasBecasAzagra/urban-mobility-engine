//! CLI module of the urban mobility engine
//!
//! Structure:
//! - [`opts`] — Argument definitions with clap (`#[derive(Parser)]`)
//! - [`commands`] — Implementation of each subcommand

pub mod commands;
pub mod opts;

pub use opts::Cli;
