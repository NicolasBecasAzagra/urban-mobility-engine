use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

/// Complete configuration of the urban mobility engine
/// Loaded from a TOML file (typically `config.toml`)
#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub geospatial: GeospatialConfig,
    pub data: DataConfig,
    pub logging: LoggingConfig,
    pub metrics: MetricsConfig,
    /// Configuration for near-real-time mode (optional)
    /// If `[live]` is not present in config.toml, this is `None`
    pub live: Option<LiveConfig>,
}

/// Parameters for the S2 geospatial grid
#[derive(Debug, Deserialize, Clone)]
pub struct GeospatialConfig {
    /// Base S2 resolution (all cells start here)
    /// Typically level 15 (≈318m)
    pub base_level: u64,

    /// High resolution for adaptive subdivision
    /// Typically level 17 (≈79m)
    pub subdivision_level: u64,

    /// delay_factor threshold that triggers subdivision
    /// Example: 1.5 = 50% slower than the base time
    pub delay_threshold: f64,

    /// Minimum number of GPS points in a cell for it to be considered valid
    pub min_gps_count: usize,
}

/// Input/output data paths
#[derive(Debug, Deserialize, Clone)]
pub struct DataConfig {
    /// Directory with raw GPS files (CSV)
    pub input_path: String,

    /// Output directory for processed data
    pub output_path: String,
}

/// Logging configuration (tracing)
#[derive(Debug, Deserialize, Clone)]
pub struct LoggingConfig {
    /// Logging level: "trace", "debug", "info", "warn", "error"
    pub level: String,
}

/// Parameters for traffic metric computation
#[derive(Debug, Deserialize, Clone)]
pub struct MetricsConfig {
    /// Reference free-flow speed for the M-30 (km/h)
    /// Free-flow speeds range between 70-100 km/h
    /// Default: 90.0
    pub m30_reference_speed: f64,

    /// Reference speed for urban roads (km/h)
    /// Used as context; the urban computation uses `carga` (load) as a proxy
    /// Default: 50.0
    pub urban_reference_speed: f64,

    /// Minimum observations per cell to generate a report
    /// Cells with fewer points produce an `InsufficientData` error
    /// Default: 5
    pub min_observations: usize,
}

/// Configuration for real-time ingestion mode
///
/// Controls the connection to the Madrid City Council traffic API
/// and the polling interval
#[derive(Debug, Deserialize, Clone)]
pub struct LiveConfig {
    /// URL of the real-time traffic API endpoint
    /// Default: `https://informo.madrid.es/informo/tmadrid/pm.xml`
    pub endpoint: String,

    /// Polling interval in seconds
    /// The Madrid API refreshes every 5 minutes (300s)
    pub poll_interval_secs: u64,

    /// Path to the locations CSV used to resolve GPS coordinates
    pub locations_path: String,
}

impl Config {
    /// Loads the configuration from a TOML file
    ///
    /// # Arguments
    /// * `path` - Path to the configuration file (e.g., "config.toml")
    ///
    /// # Errors
    /// Returns an error if:
    /// - The file does not exist or is not readable
    /// - The TOML is malformed
    /// - Required fields are missing
    /// - Validation fails (invalid S2 levels, etc.)
    ///
    /// # Example
    /// ```no_run
    /// use tfg::config::Config;
    ///
    /// let config = Config::from_file("config.toml")?;
    /// println!("Base level: {}", config.geospatial.base_level);
    /// # Ok::<(), anyhow::Error>(())
    /// ```
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();

        // Read the whole file into memory
        let contents = std::fs::read_to_string(path).with_context(|| {
            format!("Could not read the configuration file: {}", path.display())
        })?;

        // Parse the TOML and deserialize it into the Config struct
        let config: Config = toml::from_str(&contents)
            .with_context(|| format!("Error parsing TOML in: {}", path.display()))?;

        // Validate that the values make sense
        config
            .validate()
            .with_context(|| "Configuration validation failed")?;

        Ok(config)
    }

    /// Validates that the configuration is consistent
    fn validate(&self) -> Result<()> {
        // Validate that subdivision_level > base_level
        if self.geospatial.subdivision_level <= self.geospatial.base_level {
            anyhow::bail!(
                "subdivision_level ({}) must be greater than base_level ({})",
                self.geospatial.subdivision_level,
                self.geospatial.base_level
            );
        }

        // Validate that the S2 levels are in the valid range (0-30)
        if self.geospatial.base_level > 30 {
            anyhow::bail!("base_level must be between 0 and 30 (S2 maximum)");
        }
        if self.geospatial.subdivision_level > 30 {
            anyhow::bail!("subdivision_level must be between 0 and 30 (S2 maximum)");
        }

        // Validate that delay_threshold is positive
        if self.geospatial.delay_threshold <= 0.0 {
            anyhow::bail!("delay_threshold must be positive");
        }

        // Validate that min_gps_count is reasonable (> 0)
        if self.geospatial.min_gps_count == 0 {
            anyhow::bail!("min_gps_count must be at least 1");
        }

        // Validate that the logging level is valid
        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_levels.contains(&self.logging.level.as_str()) {
            anyhow::bail!("logging.level must be one of: {}", valid_levels.join(", "));
        }

        // Validate the metrics configuration
        if self.metrics.m30_reference_speed <= 0.0 {
            anyhow::bail!("metrics.m30_reference_speed must be positive");
        }
        if self.metrics.urban_reference_speed <= 0.0 {
            anyhow::bail!("metrics.urban_reference_speed must be positive");
        }
        if self.metrics.min_observations == 0 {
            anyhow::bail!("metrics.min_observations must be at least 1");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_config() {
        let toml = r#"
            [geospatial]
            base_level = 15
            subdivision_level = 17
            delay_threshold = 1.5
            min_gps_count = 50

            [data]
            input_path = "./data/gps_raw/"
            output_path = "./data/processed/"

            [logging]
            level = "info"

            [metrics]
            m30_reference_speed = 90.0
            urban_reference_speed = 50.0
            min_observations = 5
        "#;

        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.validate().is_ok());
        assert_eq!(config.geospatial.base_level, 15);
        assert_eq!(config.geospatial.subdivision_level, 17);
    }

    #[test]
    fn test_invalid_level_order() {
        let toml = r#"
            [geospatial]
            base_level = 17
            subdivision_level = 15
            delay_threshold = 1.5
            min_gps_count = 50

            [data]
            input_path = "./data/"
            output_path = "./output/"

            [logging]
            level = "info"

            [metrics]
            m30_reference_speed = 90.0
            urban_reference_speed = 50.0
            min_observations = 5
        "#;

        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_logging_level() {
        let toml = r#"
            [geospatial]
            base_level = 15
            subdivision_level = 17
            delay_threshold = 1.5
            min_gps_count = 50

            [data]
            input_path = "./data/"
            output_path = "./output/"

            [logging]
            level = "invalid"

            [metrics]
            m30_reference_speed = 90.0
            urban_reference_speed = 50.0
            min_observations = 5
        "#;

        let config: Config = toml::from_str(toml).unwrap();
        assert!(config.validate().is_err());
    }
}
