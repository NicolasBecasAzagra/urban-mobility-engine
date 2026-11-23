//! Data models for the GPS ingestion module
//!
//! Defines the structures that represent Madrid traffic data:
//! from the raw CSV record to the normalized and validated
//! GPS point

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::ingestion::error::IngestionError;

// ---------------------------------------------------------------------------
// Validation constants (Madrid bounding box)
// ---------------------------------------------------------------------------

/// Minimum latitude of the Madrid metropolitan area
pub const MADRID_LAT_MIN: f64 = 40.30;
/// Maximum latitude of the Madrid metropolitan area
pub const MADRID_LAT_MAX: f64 = 40.55;
/// Minimum longitude of the Madrid metropolitan area
pub const MADRID_LON_MIN: f64 = -3.85;
/// Maximum longitude of the Madrid metropolitan area
pub const MADRID_LON_MAX: f64 = -3.55;

/// Timestamp format used in the Madrid traffic CSVs
pub const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

// ---------------------------------------------------------------------------
// RawTrafficRecord — direct deserialization of the CSV
// ---------------------------------------------------------------------------

/// Raw record exactly as it comes from the Madrid historical traffic CSV
///
/// Fields are deserialized as `String` or `i32` because the CSV
/// uses the `;` delimiter and quotes some values. The conversion
/// to strong types is done in [`TrafficRecord::from_raw`]
#[derive(Debug, Deserialize, Clone)]
pub struct RawTrafficRecord {
    /// Measurement point identifier
    pub id: u32,
    /// Timestamp in `YYYY-MM-DD HH:MI:SS` format
    pub fecha: String,
    /// Road type: `"C30"` (M-30) or `"URB"` (urban)
    pub tipo_elem: String,
    /// Vehicles/hour (may be negative if data is missing)
    pub intensidad: i32,
    /// Detector occupancy percentage (0–100)
    pub ocupacion: i32,
    /// Degree of congestion (0=free, 100=collapse)
    pub carga: i32,
    /// Mean speed in km/h (M-30 only, 0 for urban)
    pub vmed: String,
    /// Quality flag: `"N"` normal, `"E"` error, `"S"` substituted
    pub error: String,
    /// Number of samples received in the 15 min period (1–5)
    pub periodo_integracion: u8,
}

// ---------------------------------------------------------------------------
// DataQuality — record quality
// ---------------------------------------------------------------------------

/// Quality of a traffic record according to the CSV `error` flag
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataQuality {
    /// Normal data, no errors detected
    Normal,
    /// Data with an error in the sensor parameters
    Error,
    /// Erroneous sample replaced by an estimate
    Substituted,
}

impl DataQuality {
    /// Converts the CSV `error` field string to a `DataQuality`
    ///
    /// # Arguments
    /// * `flag` - Value of the `error` field: `"N"`, `"E"` or `"S"`
    ///
    /// # Errors
    /// Returns an error if the flag is not one of the expected values
    pub fn from_flag(flag: &str) -> Result<Self, IngestionError> {
        let trimmed = flag.trim().trim_matches('"');
        match trimmed {
            "N" | "" => Ok(DataQuality::Normal),
            "E" => Ok(DataQuality::Error),
            "S" => Ok(DataQuality::Substituted),
            other => Err(IngestionError::InvalidField {
                field: "error".to_string(),
                reason: format!("unknown flag: '{}'", other),
            }),
        }
    }

    /// Indicates whether the data is reliable for analysis
    pub fn is_reliable(&self) -> bool {
        matches!(self, DataQuality::Normal)
    }
}

// ---------------------------------------------------------------------------
// TrafficRecord — normalized and validated record
// ---------------------------------------------------------------------------

/// Normalized traffic record with strong types
///
/// Obtained from a [`RawTrafficRecord`] through [`TrafficRecord::from_raw`],
/// which parses the timestamp, validates ranges and converts the fields
#[derive(Debug, Clone)]
pub struct TrafficRecord {
    /// Measurement point identifier
    pub id: u32,
    /// Parsed timestamp
    pub timestamp: NaiveDateTime,
    /// Road type
    pub road_type: RoadType,
    /// Vehicles/hour in the 15 min period
    pub intensidad: i32,
    /// Detector occupancy percentage
    pub ocupacion: i32,
    /// Degree of congestion
    pub carga: i32,
    /// Mean speed in km/h (only available on the M-30)
    pub velocidad: Option<i32>,
    /// Data quality
    pub quality: DataQuality,
    /// Number of samples in the period
    pub periodo_integracion: u8,
}

/// Type of road where the measurement point is located
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoadType {
    /// Conventional urban road
    Urban,
    /// M-30 motorway
    M30,
}

impl RoadType {
    /// Converts the CSV `tipo_elem` field to a `RoadType`
    pub fn parse(s: &str) -> Result<Self, IngestionError> {
        match s.trim().trim_matches('"') {
            "URB" | "Urbano" => Ok(RoadType::Urban),
            "C30" | "M30" => Ok(RoadType::M30),
            other => Err(IngestionError::InvalidField {
                field: "tipo_elem".to_string(),
                reason: format!("unknown type: '{}'", other),
            }),
        }
    }
}

impl TrafficRecord {
    /// Creates a normalized `TrafficRecord` from a raw CSV record
    ///
    /// Performs the following conversions and validations:
    /// - Timestamp parsing
    /// - Road type conversion
    /// - Speed: `Some(v)` for the M-30, `None` for urban
    /// - Quality flag parsing
    ///
    /// # Errors
    /// Returns an error if the timestamp is not parseable or if any field
    /// has an unknown value
    pub fn from_raw(raw: &RawTrafficRecord) -> Result<Self, IngestionError> {
        // Parse the timestamp (the CSV quotes the value)
        let fecha_clean = raw.fecha.trim().trim_matches('"');
        let timestamp =
            NaiveDateTime::parse_from_str(fecha_clean, TIMESTAMP_FORMAT).map_err(|e| {
                IngestionError::InvalidTimestamp {
                    value: raw.fecha.clone(),
                    source: e,
                }
            })?;

        // Road type
        let road_type = RoadType::parse(&raw.tipo_elem)?;

        // Speed: only meaningful on the M-30
        let parsed_vmed: i32 = raw.vmed.trim().trim_matches('"').parse().unwrap_or(0);
        let velocidad = match road_type {
            RoadType::M30 if parsed_vmed > 0 => Some(parsed_vmed),
            _ => None,
        };

        // Data quality
        let quality = DataQuality::from_flag(raw.error.trim().trim_matches('"'))?;

        Ok(TrafficRecord {
            id: raw.id,
            timestamp,
            road_type,
            intensidad: raw.intensidad,
            ocupacion: raw.ocupacion,
            carga: raw.carga,
            velocidad,
            quality,
            periodo_integracion: raw.periodo_integracion,
        })
    }

    /// Indicates whether the record has valid (non-negative) data
    ///
    /// Negative intensity/occupancy values indicate the sensor had
    /// no data in that period
    pub fn has_valid_data(&self) -> bool {
        self.intensidad >= 0 && self.ocupacion >= 0 && self.carga >= 0
    }
}

// ---------------------------------------------------------------------------
// Location — geographic location of a measurement point
// ---------------------------------------------------------------------------

/// Geographic location of a traffic measurement point
///
/// Loaded from the locations CSV (originally a Shapefile)
/// Contains the WGS84 coordinates already converted
#[derive(Debug, Deserialize, Clone)]
pub struct Location {
    /// Measurement point identifier (JOIN field with the traffic CSV)
    pub id: u32,
    /// Road element type
    pub tipo_elem: String,
    /// Madrid district code (1–21)
    pub distrito: Option<u32>,
    /// Descriptive name of the measurement point
    pub nombre: String,
    /// WGS84 longitude in decimal degrees
    pub longitud: f64,
    /// WGS84 latitude in decimal degrees
    pub latitud: f64,
}

impl Location {
    /// Validates that the coordinates are inside the Madrid bounding box
    pub fn validate_bounds(&self) -> Result<(), IngestionError> {
        if self.latitud < MADRID_LAT_MIN || self.latitud > MADRID_LAT_MAX {
            return Err(IngestionError::OutOfBounds {
                lat: self.latitud,
                lon: self.longitud,
            });
        }
        if self.longitud < MADRID_LON_MIN || self.longitud > MADRID_LON_MAX {
            return Err(IngestionError::OutOfBounds {
                lat: self.latitud,
                lon: self.longitud,
            });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// GpsPoint — integrated GPS point (traffic + location)
// ---------------------------------------------------------------------------

/// Integrated GPS point combining the temporal traffic data
/// with its geographic location
///
/// This is the final result of the ingestion pipeline: a georeferenced,
/// normalized and validated traffic record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpsPoint {
    /// Measurement point identifier
    pub id: u32,
    /// Observation timestamp
    pub timestamp: NaiveDateTime,
    /// WGS84 latitude
    pub latitude: f64,
    /// WGS84 longitude
    pub longitude: f64,
    /// Road type
    pub road_type: RoadType,
    /// Vehicles/hour
    pub intensidad: i32,
    /// Detector occupancy percentage
    pub ocupacion: i32,
    /// Degree of congestion
    pub carga: i32,
    /// Mean speed in km/h (M-30 only)
    pub velocidad: Option<i32>,
    /// Data quality
    pub quality: DataQuality,
}

impl GpsPoint {
    /// Combines a `TrafficRecord` with its `Location` to create a `GpsPoint`
    pub fn from_record_and_location(record: &TrafficRecord, location: &Location) -> Self {
        GpsPoint {
            id: record.id,
            timestamp: record.timestamp,
            latitude: location.latitud,
            longitude: location.longitud,
            road_type: record.road_type,
            intensidad: record.intensidad,
            ocupacion: record.ocupacion,
            carga: record.carga,
            velocidad: record.velocidad,
            quality: record.quality,
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_raw_record() -> RawTrafficRecord {
        RawTrafficRecord {
            id: 1006,
            fecha: "2025-12-01 00:00:00".to_string(),
            tipo_elem: "C30".to_string(),
            intensidad: 240,
            ocupacion: 0,
            carga: 0,
            vmed: "70".to_string(),
            error: "N".to_string(),
            periodo_integracion: 5,
        }
    }

    fn sample_raw_urban() -> RawTrafficRecord {
        RawTrafficRecord {
            id: 3950,
            fecha: "2025-12-01 00:00:00".to_string(),
            tipo_elem: "URB".to_string(),
            intensidad: 120,
            ocupacion: 5,
            carga: 10,
            vmed: "0".to_string(),
            error: "N".to_string(),
            periodo_integracion: 5,
        }
    }

    #[test]
    fn test_traffic_record_from_raw_m30() {
        let raw = sample_raw_record();
        let record = TrafficRecord::from_raw(&raw).unwrap();

        assert_eq!(record.id, 1006);
        assert_eq!(record.road_type, RoadType::M30);
        assert_eq!(record.velocidad, Some(70));
        assert!(record.quality.is_reliable());
        assert!(record.has_valid_data());
    }

    #[test]
    fn test_traffic_record_from_raw_urban() {
        let raw = sample_raw_urban();
        let record = TrafficRecord::from_raw(&raw).unwrap();

        assert_eq!(record.id, 3950);
        assert_eq!(record.road_type, RoadType::Urban);
        assert_eq!(record.velocidad, None);
    }

    #[test]
    fn test_traffic_record_invalid_timestamp() {
        let mut raw = sample_raw_record();
        raw.fecha = "INVALID-DATE".to_string();

        let result = TrafficRecord::from_raw(&raw);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid timestamp"));
    }

    #[test]
    fn test_traffic_record_negative_values() {
        let mut raw = sample_raw_record();
        raw.intensidad = -1;
        raw.ocupacion = -1;

        let record = TrafficRecord::from_raw(&raw).unwrap();
        assert!(!record.has_valid_data());
    }

    #[test]
    fn test_data_quality_from_flag() {
        assert_eq!(DataQuality::from_flag("N").unwrap(), DataQuality::Normal);
        assert_eq!(DataQuality::from_flag("E").unwrap(), DataQuality::Error);
        assert_eq!(
            DataQuality::from_flag("S").unwrap(),
            DataQuality::Substituted
        );
        assert!(DataQuality::from_flag("X").is_err());
    }

    #[test]
    fn test_data_quality_reliability() {
        assert!(DataQuality::Normal.is_reliable());
        assert!(!DataQuality::Error.is_reliable());
        assert!(!DataQuality::Substituted.is_reliable());
    }

    #[test]
    fn test_road_type_from_str() {
        assert_eq!(RoadType::parse("URB").unwrap(), RoadType::Urban);
        assert_eq!(RoadType::parse("Urbano").unwrap(), RoadType::Urban);
        assert_eq!(RoadType::parse("C30").unwrap(), RoadType::M30);
        assert_eq!(RoadType::parse("M30").unwrap(), RoadType::M30);
        assert!(RoadType::parse("UNKNOWN").is_err());
    }

    #[test]
    fn test_location_validate_bounds_valid() {
        let loc = Location {
            id: 10011,
            tipo_elem: "URB".to_string(),
            distrito: Some(9),
            nombre: "Test".to_string(),
            longitud: -3.72,
            latitud: 40.45,
        };
        assert!(loc.validate_bounds().is_ok());
    }

    #[test]
    fn test_location_validate_bounds_invalid_lat() {
        let loc = Location {
            id: 10011,
            tipo_elem: "URB".to_string(),
            distrito: Some(9),
            nombre: "Test".to_string(),
            longitud: -3.72,
            latitud: 41.0, // Outside Madrid
        };
        assert!(loc.validate_bounds().is_err());
    }

    #[test]
    fn test_gps_point_creation() {
        let raw = sample_raw_record();
        let record = TrafficRecord::from_raw(&raw).unwrap();
        let location = Location {
            id: 1006,
            tipo_elem: "M30".to_string(),
            distrito: Some(14),
            nombre: "PM20001".to_string(),
            longitud: -3.66153,
            latitud: 40.41256,
        };

        let point = GpsPoint::from_record_and_location(&record, &location);
        assert_eq!(point.id, 1006);
        assert!((point.latitude - 40.41256).abs() < 0.0001);
        assert!((point.longitude - (-3.66153)).abs() < 0.0001);
        assert_eq!(point.road_type, RoadType::M30);
    }
}
