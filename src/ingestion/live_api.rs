//! Real-time data source from the Madrid traffic API
//!
//! Implements [`DataSource`](super::datasource::DataSource) for the public
//! Madrid City Council API, which publishes traffic data
//! every 5 minutes in XML format
//!
//! ## Endpoint
//!
//! `https://informo.madrid.es/informo/tmadrid/pm.xml`
//!
//! ## XML schema
//!
//! ```xml
//! <pms>
//!   <fecha_hora>DD/MM/YYYY H:MM:SS</fecha_hora>
//!   <pm>
//!     <idelem>ID</idelem>
//!     <descripcion>Street name and direction</descripcion>
//!     <intensidad>veh/h</intensidad>
//!     <ocupacion>0-100</ocupacion>
//!     <carga>0-100</carga>
//!     <velocidad>km/h (M-30 only)</velocidad>  <!-- optional -->
//!     <error>N/E/S</error>
//!     <st_x>UTM X</st_x>
//!     <st_y>UTM Y</st_y>
//!   </pm>
//! </pms>
//! ```
//!
//! ## Example
//!
//! ```rust,no_run
//! use tfg::ingestion::live_api::LiveApiSource;
//! use tfg::ingestion::datasource::DataSource;
//!
//! let mut source = LiveApiSource::new(
//!     "https://informo.madrid.es/informo/tmadrid/pm.xml",
//!     "data/samples/locations.csv",
//! ).unwrap();
//!
//! let points = source.fetch_batch().unwrap();
//! println!("Received {} real-time points", points.len());
//! ```

use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::NaiveDateTime;

use crate::ingestion::csv_parser;
use crate::ingestion::datasource::DataSource;
use crate::ingestion::error::IngestionError;
use crate::ingestion::model::{DataQuality, GpsPoint, Location, RoadType};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// API timestamp format (DD/MM/YYYY H:MM:SS)
const API_TIMESTAMP_FORMAT: &str = "%d/%m/%Y %H:%M:%S";

/// Alternative format with a 2-digit hour
const API_TIMESTAMP_FORMAT_ALT: &str = "%d/%m/%Y %k:%M:%S";

/// Timeout for HTTP requests (in seconds)
const HTTP_TIMEOUT_SECS: u64 = 30;

/// Retry interval (in seconds) when the API has not refreshed yet
/// The Madrid API refreshes every 5 min; with 30s we get up to 10 retries
const RETRY_INTERVAL_SECS: u64 = 30;

// ---------------------------------------------------------------------------
// LiveApiSource
// ---------------------------------------------------------------------------

/// Real-time data source from the Madrid traffic API
///
/// Performs HTTP GET requests to the endpoint and parses the XML response
/// to produce `GpsPoint`s. Coordinates are resolved through
/// the locations CSV (same as for historical data)
pub struct LiveApiSource {
    /// API endpoint URL
    endpoint: String,
    /// HashMap of locations indexed by ID (to resolve coordinates)
    locations: HashMap<u32, Location>,
    /// Reusable HTTP client
    client: reqwest::blocking::Client,
    /// Timestamp of the last successful HTTP request (monotonic clock)
    last_fetch: Option<Instant>,
    /// Polling interval in seconds
    poll_interval: Duration,
    /// `<fecha_hora>` timestamp of the last XML processed with new data
    ///
    /// Used to detect whether the API has refreshed its data between two
    /// consecutive `fetch_batch` calls. If the timestamp does not change,
    /// the data is identical to the previous tick and is discarded
    last_api_timestamp: Option<NaiveDateTime>,
}

impl LiveApiSource {
    /// Creates a new data source for the real-time API
    ///
    /// Loads the locations from the given CSV and configures
    /// the HTTP client with a timeout
    ///
    /// # Arguments
    /// * `endpoint` — URL of the XML traffic API
    /// * `locations_path` — Path to the locations CSV
    ///
    /// # Errors
    /// Returns an error if the locations cannot be loaded
    pub fn new(endpoint: &str, locations_path: &str) -> Result<Self, IngestionError> {
        let locations = csv_parser::load_locations_csv(locations_path).map_err(|e| {
            IngestionError::InvalidField {
                field: "locations_path".to_string(),
                reason: format!("Error loading locations: {}", e),
            }
        })?;

        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build()
            .map_err(|e| {
                IngestionError::HttpRequest(format!("Error creating HTTP client: {}", e))
            })?;

        tracing::info!(
            "LiveApiSource created: endpoint='{}', {} locations loaded",
            endpoint,
            locations.len()
        );

        Ok(LiveApiSource {
            endpoint: endpoint.to_string(),
            locations,
            client,
            last_fetch: None,
            poll_interval: Duration::from_secs(300), // 5 minutes by default
            last_api_timestamp: None,
        })
    }

    /// Returns the `<fecha_hora>` timestamp of the last XML processed with new data
    ///
    /// Useful for inspection in tests and in the TUI (to show when the last
    /// real API update happened, not when the fetch was launched)
    pub fn last_api_timestamp(&self) -> Option<NaiveDateTime> {
        self.last_api_timestamp
    }

    /// Polls with retries until data with a new `<fecha_hora>` is obtained
    ///
    /// If the API has not refreshed yet (the XML timestamp matches the
    /// previous tick's), waits [`RETRY_INTERVAL_SECS`] seconds and retries
    /// The maximum number of retries is bounded by `max_retries`
    ///
    /// # Behavior
    /// - If it obtains new data → returns it immediately
    /// - If it exhausts the retries without new data → returns `Ok(vec![])` with
    ///   a log warning (not an error; there is simply nothing new)
    pub fn fetch_if_new(&mut self, max_retries: u32) -> Result<Vec<GpsPoint>, IngestionError> {
        for attempt in 0..=max_retries {
            let body = self.fetch_raw_body()?;
            let timestamp = self.extract_timestamp(body.trim_start_matches('\u{feff}').trim())?;

            if self.last_api_timestamp == Some(timestamp) {
                if attempt < max_retries {
                    tracing::warn!(
                        "LiveApiSource: unchanged data (fecha_hora={}), retry {}/{} in {}s",
                        timestamp,
                        attempt + 1,
                        max_retries,
                        RETRY_INTERVAL_SECS
                    );
                    std::thread::sleep(Duration::from_secs(RETRY_INTERVAL_SECS));
                    continue;
                } else {
                    tracing::warn!(
                        "LiveApiSource: API did not refresh after {} retries (fecha_hora={}). \
                         Tick discarded.",
                        max_retries,
                        timestamp
                    );
                    return Ok(vec![]);
                }
            }

            // New timestamp → parse and update state
            tracing::info!(
                "LiveApiSource: new data detected (fecha_hora={}, previous={:?})",
                timestamp,
                self.last_api_timestamp
            );
            self.last_api_timestamp = Some(timestamp);
            self.last_fetch = Some(Instant::now());
            return self.parse_xml_response(&body);
        }
        Ok(vec![])
    }

    /// Performs the HTTP request and returns the body as a String
    fn fetch_raw_body(&self) -> Result<String, IngestionError> {
        tracing::info!("LiveApiSource: GET request to '{}'", self.endpoint);
        let response = self
            .client
            .get(&self.endpoint)
            .send()
            .map_err(|e| IngestionError::HttpRequest(format!("Request error: {}", e)))?;

        if !response.status().is_success() {
            return Err(IngestionError::HttpRequest(format!(
                "HTTP {}: {}",
                response.status(),
                response.status().canonical_reason().unwrap_or("Error")
            )));
        }

        response
            .text()
            .map_err(|e| IngestionError::HttpRequest(format!("Error reading body: {}", e)))
    }

    /// Creates a LiveApiSource with a custom polling interval
    pub fn with_poll_interval(mut self, secs: u64) -> Self {
        self.poll_interval = Duration::from_secs(secs);
        self
    }

    /// Parses an XML response from the Madrid API
    ///
    /// Extracts the fields of each `<pm>` and combines them with the
    /// locations to produce `GpsPoint`s
    ///
    /// # Arguments
    /// * `xml_body` — XML body of the HTTP response
    ///
    /// # Returns
    /// Vector of valid `GpsPoint`s (points without a location are discarded)
    pub fn parse_xml_response(&self, xml_body: &str) -> Result<Vec<GpsPoint>, IngestionError> {
        // Strip the BOM if present (the Madrid API includes it)
        let xml_clean = xml_body.trim_start_matches('\u{feff}').trim();

        // Parse the global timestamp
        let timestamp = self.extract_timestamp(xml_clean)?;

        // Parse each <pm> element
        let mut points = Vec::new();
        let mut total_parsed = 0u32;
        let mut missing_location = 0u32;
        let mut error_flagged = 0u32;

        // Iterate over each <pm>...</pm> block
        let mut search_from = 0;
        while let Some(pm_start) = xml_clean[search_from..].find("<pm>") {
            let pm_start = search_from + pm_start;
            let pm_end = match xml_clean[pm_start..].find("</pm>") {
                Some(pos) => pm_start + pos + 5,
                None => break,
            };
            let pm_block = &xml_clean[pm_start..pm_end];
            search_from = pm_end;

            total_parsed += 1;

            match self.parse_pm_block(pm_block, timestamp) {
                Ok(Some(point)) => points.push(point),
                Ok(None) => missing_location += 1,
                Err(_) => error_flagged += 1,
            }
        }

        tracing::info!(
            "LiveApiSource: parsed {} XML points → {} GpsPoints ({} without location, {} with error)",
            total_parsed,
            points.len(),
            missing_location,
            error_flagged
        );

        Ok(points)
    }

    /// Extracts the global `<fecha_hora>` timestamp from the XML
    fn extract_timestamp(&self, xml: &str) -> Result<NaiveDateTime, IngestionError> {
        let fecha = extract_tag_value(xml, "fecha_hora").ok_or_else(|| {
            IngestionError::XmlParse("<fecha_hora> not found in the response".to_string())
        })?;

        // Try both formats (hour with/without padding)
        NaiveDateTime::parse_from_str(fecha, API_TIMESTAMP_FORMAT)
            .or_else(|_| NaiveDateTime::parse_from_str(fecha.trim(), API_TIMESTAMP_FORMAT_ALT))
            .map_err(|e| IngestionError::XmlParse(format!("Invalid timestamp '{}': {}", fecha, e)))
    }

    /// Parses an individual `<pm>...</pm>` block
    ///
    /// Returns:
    /// - `Ok(Some(point))` — Valid point with a location
    /// - `Ok(None)` — Point without a location in the CSV (discarded)
    /// - `Err` — Parse error or data with an error flag
    fn parse_pm_block(
        &self,
        block: &str,
        timestamp: NaiveDateTime,
    ) -> Result<Option<GpsPoint>, IngestionError> {
        // Extract the required fields
        let id: u32 = extract_tag_value(block, "idelem")
            .ok_or_else(|| IngestionError::XmlParse("Missing <idelem>".to_string()))?
            .parse()
            .map_err(|e| IngestionError::XmlParse(format!("Non-numeric idelem: {}", e)))?;

        let intensidad: i32 = parse_xml_int(block, "intensidad")?;
        let ocupacion: i32 = parse_xml_int(block, "ocupacion")?;
        let carga: i32 = parse_xml_int(block, "carga")?;

        // Speed: optional (M-30 only)
        let velocidad: Option<i32> = extract_tag_value(block, "velocidad")
            .and_then(|v| v.parse().ok())
            .filter(|&v| v > 0);

        // Error flag
        let error_flag = extract_tag_value(block, "error").unwrap_or("N");
        let quality = DataQuality::from_flag(error_flag)?;

        // Determine the road type by the presence of speed
        let road_type = if velocidad.is_some() {
            RoadType::M30
        } else {
            RoadType::Urban
        };

        // Look up the location by ID
        let location = match self.locations.get(&id) {
            Some(loc) => loc,
            None => return Ok(None), // No location → discard
        };

        Ok(Some(GpsPoint {
            id,
            timestamp,
            latitude: location.latitud,
            longitude: location.longitud,
            road_type,
            intensidad,
            ocupacion,
            carga,
            velocidad,
            quality,
        }))
    }

    /// Returns the number of loaded locations
    pub fn location_count(&self) -> usize {
        self.locations.len()
    }
}

impl DataSource for LiveApiSource {
    fn name(&self) -> &str {
        "Madrid Real-Time API"
    }

    fn fetch_batch(&mut self) -> Result<Vec<GpsPoint>, IngestionError> {
        // Use fetch_if_new with up to 9 retries (9 × 30s = 270s < 5 min)
        // This guarantees we always process fresh data, regardless of
        // when the first tick was launched relative to the Madrid API cycle
        self.fetch_if_new(9)
    }

    fn has_more(&self) -> bool {
        true // The API always has new data
    }

    fn refresh_interval(&self) -> Option<Duration> {
        Some(self.poll_interval)
    }
}

// ---------------------------------------------------------------------------
// XML parsing helpers
// ---------------------------------------------------------------------------

/// Extracts the text value of a simple XML tag
///
/// For `<tag>value</tag>` returns `Some("value")`
/// Does not support attributes or nested tags
fn extract_tag_value<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open_tag = format!("<{}>", tag);
    let close_tag = format!("</{}>", tag);

    let start = xml.find(&open_tag)? + open_tag.len();
    let end = xml[start..].find(&close_tag)? + start;

    Some(&xml[start..end])
}

/// Parses an XML field as an i32
fn parse_xml_int(block: &str, tag: &str) -> Result<i32, IngestionError> {
    let value = extract_tag_value(block, tag)
        .ok_or_else(|| IngestionError::XmlParse(format!("Missing <{}>", tag)))?;

    value.trim().parse().map_err(|e| {
        IngestionError::XmlParse(format!("<{}> is not numeric '{}': {}", tag, value, e))
    })
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_LOCATIONS: &str = "tests/fixtures/locations.csv";
    const FIXTURE_XML: &str = "tests/fixtures/sample_api_response.xml";

    /// Creates a LiveApiSource for tests (with a dummy endpoint)
    fn test_source() -> LiveApiSource {
        LiveApiSource::new("http://localhost:9999/fake", FIXTURE_LOCATIONS).unwrap()
    }

    // --- XML parser tests ---

    #[test]
    fn test_parse_xml_response_basic() {
        let source = test_source();
        let xml = std::fs::read_to_string(FIXTURE_XML).unwrap();
        let points = source.parse_xml_response(&xml).unwrap();

        // The fixture has 5 points, but only 3 IDs have a location
        // in locations.csv (3950, 10011, 1006). IDs 9999 and 5555 do not
        // Also, ID 5555 has error=E, which is still valid
        // (DataQuality::Error is parsed correctly)
        // Only 9999 and 5555 have no location → they are discarded
        assert!(!points.is_empty());
        assert_eq!(points.len(), 3, "3 IDs with a location in locations.csv");
    }

    #[test]
    fn test_parse_xml_point_details() {
        let source = test_source();
        let xml = std::fs::read_to_string(FIXTURE_XML).unwrap();
        let points = source.parse_xml_response(&xml).unwrap();

        // Find the M-30 point (ID 1006, has speed)
        let m30_point = points.iter().find(|p| p.id == 1006).unwrap();
        assert_eq!(m30_point.road_type, RoadType::M30);
        assert_eq!(m30_point.velocidad, Some(72));
        assert_eq!(m30_point.intensidad, 2400);
        assert_eq!(m30_point.carga, 65);

        // Find the urban point (ID 3950)
        let urban_point = points.iter().find(|p| p.id == 3950).unwrap();
        assert_eq!(urban_point.road_type, RoadType::Urban);
        assert_eq!(urban_point.velocidad, None);
        assert_eq!(urban_point.intensidad, 380);
    }

    #[test]
    fn test_parse_xml_timestamp() {
        let source = test_source();
        let xml = std::fs::read_to_string(FIXTURE_XML).unwrap();
        let points = source.parse_xml_response(&xml).unwrap();

        // All points must share the same (global) timestamp
        let ts = points[0].timestamp;
        for point in &points {
            assert_eq!(point.timestamp, ts, "All share the global timestamp");
        }
        assert_eq!(ts.format("%Y-%m-%d").to_string(), "2026-04-08");
    }

    #[test]
    fn test_parse_xml_coordinates() {
        let source = test_source();
        let xml = std::fs::read_to_string(FIXTURE_XML).unwrap();
        let points = source.parse_xml_response(&xml).unwrap();

        // Coordinates come from the locations CSV, not from the XML
        for point in &points {
            assert!(point.latitude > 40.0, "Latitude must be in Madrid");
            assert!(point.longitude < -3.0, "Longitude must be in Madrid");
        }
    }

    #[test]
    fn test_parse_xml_empty() {
        let source = test_source();
        let xml = r#"<?xml version="1.0"?>
            <pms>
                <fecha_hora>01/01/2026 12:00:00</fecha_hora>
            </pms>"#;

        let points = source.parse_xml_response(xml).unwrap();
        assert!(points.is_empty());
    }

    #[test]
    fn test_parse_xml_missing_fecha() {
        let source = test_source();
        let xml = "<pms><pm><idelem>1</idelem></pm></pms>";

        let result = source.parse_xml_response(xml);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("fecha_hora"));
    }

    #[test]
    fn test_parse_xml_bom() {
        let source = test_source();
        // The Madrid API includes a UTF-8 BOM at the start
        let xml = "\u{feff}<pms><fecha_hora>01/01/2026 12:00:00</fecha_hora></pms>";

        let points = source.parse_xml_response(xml).unwrap();
        assert!(points.is_empty()); // No <pm>, but it parses OK
    }

    #[test]
    fn test_parse_xml_with_error_flag() {
        let source = test_source();
        let xml = r#"<pms>
            <fecha_hora>01/01/2026 12:00:00</fecha_hora>
            <pm>
                <idelem>3950</idelem>
                <intensidad>0</intensidad>
                <ocupacion>0</ocupacion>
                <carga>0</carga>
                <error>E</error>
            </pm>
        </pms>"#;

        let points = source.parse_xml_response(xml).unwrap();
        // A point with error=E is still parsed (DataQuality::Error)
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].quality, DataQuality::Error);
    }

    // --- Helper tests ---

    #[test]
    fn test_extract_tag_value() {
        let xml = "<root><nombre>Madrid</nombre><id>42</id></root>";
        assert_eq!(extract_tag_value(xml, "nombre"), Some("Madrid"));
        assert_eq!(extract_tag_value(xml, "id"), Some("42"));
        assert_eq!(extract_tag_value(xml, "missing"), None);
    }

    #[test]
    fn test_parse_xml_int_valid() {
        let block = "<pm><intensidad>500</intensidad></pm>";
        assert_eq!(parse_xml_int(block, "intensidad").unwrap(), 500);
    }

    #[test]
    fn test_parse_xml_int_missing() {
        let block = "<pm><other>500</other></pm>";
        assert!(parse_xml_int(block, "intensidad").is_err());
    }

    // --- DataSource trait tests ---

    #[test]
    fn test_live_source_creation() {
        let source = test_source();
        assert_eq!(source.name(), "Madrid Real-Time API");
        assert!(source.has_more());
        assert!(source.refresh_interval().is_some());
        assert_eq!(source.refresh_interval().unwrap(), Duration::from_secs(300));
        assert_eq!(source.location_count(), 3);
    }

    #[test]
    fn test_live_source_as_trait_object() {
        let source: Box<dyn DataSource> = Box::new(test_source());
        assert!(source.has_more());
        assert_eq!(source.name(), "Madrid Real-Time API");
    }

    #[test]
    fn test_live_source_custom_interval() {
        let source = test_source().with_poll_interval(60);
        assert_eq!(source.refresh_interval().unwrap(), Duration::from_secs(60));
    }
}
