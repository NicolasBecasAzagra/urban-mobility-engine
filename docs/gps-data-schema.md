# Data Schema - Madrid Traffic

**Analysis date:** February 2, 2026  
**Datasets analyzed:** Historical December 2025 + Locations December 2025  
**Sample analyzed:** 1,000 rows of the historical data

---

## Executive Summary

**Data sources:** 2 complementary files related by `ID`

| File | Format | Records | Purpose |
|------|--------|---------|---------|
| `12-2025.csv` | CSV (`;` delimited) | 11M+ (full month) | Temporal traffic data |
| `pmed_ubicacion_12-2025/` | Shapefile (GIS) | 5,024 points | Georeferenced locations |

**Integration strategy:** JOIN on the `ID` field

---

## 1. Historical Traffic CSV

### Location
`data/raw/12-2025.csv`

### Technical Characteristics
- **Format:** CSV with `;` (semicolon) delimiter
- **Encoding:** UTF-8
- **Size:** ~765 MB (December 2025)
- **Total records:** ~11 million
- **Temporal granularity:** 15 minutes
- **Period:**  2025-12-01 00:00:00 to 2025-12-31 23:45:00

### CSV Fields

| Field | Type | Description | Example | Range/Values |
|-------|------|-------------|---------|--------------|
| `id` | Integer | Unique identifier of the measurement point | 1006 | 1-99999 |
| `fecha` | String | Timestamp in `YYYY-MM-DD HH:MI:SS` format | 2025-12-01 00:00:00 | Timestamps every 15 min |
| `tipo_elem` | String | Traffic element type | C30 | `Urbano` (urban), `C30` (M-30) |
| `intensidad` | Integer | Vehicles/hour over a 15-min period | 732 | 0 - 2736 veh/h |
| `ocupacion` | Integer | % of time the detector is occupied | 2 | 0 - 100% |
| `carga` | Integer | Congestion level (0=empty, 100=collapse) | 0 | 0 - 100 |
| `vmed` | Integer | Average speed (km/h) - **M-30 only** | 63 | 0 - 150 km/h |
| `error` | String | Data quality | N | `N` (normal), `E` (error), `S` (substituted) |
| `periodo_integracion` | Integer | Number of samples received in 15 min | 5 | 1 - 5 |

### Sample Statistics (1,000 rows, point 1006 M-30)

```
Intensity:
  Mean: 746 veh/h
  Min: 0 veh/h
  25%: 312 veh/h
  50%: 732 veh/h (median)
  75%: 1,068 veh/h
  Max: 2,736 veh/h

Occupancy:
  Mean: 2%
  Range: 0 - 7%

Speed:
  Mean: 63 km/h
  Range: 0 - 83 km/h
```

### Timestamp Format

**Format:** `YYYY-MM-DD HH:MI:SS`

**Parsing in Rust (chrono):**
```rust
use chrono::NaiveDateTime;

let timestamp = NaiveDateTime::parse_from_str(
    "2025-12-01 00:15:00",
    "%Y-%m-%d %H:%M:%S"
)?;
```

### Value Conversion

#### Intensity → Actual vehicles in 15 min
```rust
let vehiculos_15min = intensidad / 4;
// Example: 732 veh/h → 183 vehicles in 15 minutes
```

#### Occupancy (%)
- **0%:** Detector free the whole time
- **50%:** Vehicles over the detector 7.5 min out of every 15 min
- **100%:** Detector always occupied (total congestion)

### Data Quality

**Null values in sample:** 0 (excellent)

**Error flags:**
- `N` (Normal): 100% of the sample ✅
- `E` (Parameter error): Not found in the sample
- `S` (Erroneous sample substituted): Not found in the sample

**Negative values:** Indicate absence of data (not found in the analyzed sample)

---

## 2. Locations Shapefile

### Location
`data/raw/pmed_ubicacion_12-2025/pmed_ubicacion_12-2025.shp` (+ auxiliary files)

### Technical Characteristics
- **Format:** ESRI Shapefile (standard GIS format)
- **Coordinate system:** EPSG:25830 (ETRS89 / UTM zone 30N)
- **Geometry:** Polygons (detection area)
- **Records:** 5,024 measurement points

### Shapefile Fields

| Field | Type | Description | Example | Notes |
|-------|------|-------------|---------|-------|
| `ID` | Integer | Unique identifier (JOIN field) | 10011 | Matches the CSV `id` |
| `TIPO_ELEM` | String | Element type | URB | `URB`, `M30`, `other` |
| `DISTRITO` | Integer | Madrid district code | 1 | 1-21 (may be null) |
| `COD_CENT` | String | Central controller code | 240102 | Internal code |
| `NOMBRE` | String | Description of the measurement point | ANTONIO NOVAIS O-E... | Street + direction |
| `UTM_X` | Float | X coordinate in UTM (meters) | 438497.626 | ETRS89 system |
| `UTM_Y` | Float | Y coordinate in UTM (meters) | 4477793.706 | ETRS89 system |
| `LONGITUD` | Float | WGS84 longitude (decimal degrees) | -3.725273 | **Already converted** ✅ |
| `LATITUD` | Float | WGS84 latitude (decimal degrees) | 40.448526 | **Already converted** ✅ |
| `geometry` | Geometry | Detection-area polygon | POLYGON(...) | GIS geometry |

### Distribution of Measurement Points

```
Total: 5,024 points

By type:
  URB (Urban):     4,585 (91.3%)
  M30 (Highway):     314 (6.2%)
  other:             125 (2.5%)
```

### Geographic Ranges (Madrid)

#### UTM coordinates (ETRS89, meters)
```
X (East):  429,060.79 - 453,194.52
Y (North): 4,464,901.55 - 4,485,212.99
```

#### WGS84 coordinates (Lat/Lon, degrees)
```
Latitude:  40.332454°N - 40.515611°N
Longitude: -3.836886°W  - -3.551623°W
```

**Approximate area:** Madrid metropolitan region (~24 km × 20 km)

---

## 3. Source Integration

### JOIN Strategy

**Key field:** `ID` (shapefile) ↔ `id` (historical CSV)

**Relationship:** 1:N
- 1 location point → N temporal records (one every 15 min)

### Integration Algorithm (Rust)

```rust
use std::collections::HashMap;

// 1. Load locations into a HashMap (one-time load)
let locations: HashMap<u32, Location> = load_locations_shapefile(
    "data/raw/pmed_ubicacion_12-2025/"
)?;

// 2. Read the historical CSV with streaming (avoids loading 11M rows into RAM)
let reader = csv::Reader::from_path("data/raw/12-2025.csv")?;

for result in reader.deserialize() {
    let record: TrafficRecord = result?;
    
    // 3. Location lookup (O(1) with HashMap)
    if let Some(location) = locations.get(&record.id) {
        let traffic_point = TrafficPoint {
            id: record.id,
            timestamp: parse_timestamp(&record.fecha)?,
            coords: (location.latitud, location.longitud), // Already in WGS84
            metrics: TrafficMetrics {
                intensidad: record.intensidad,
                ocupacion: record.ocupacion,
                velocidad: record.vmed,
                carga: record.carga,
            },
            quality: DataQuality::from_error(&record.error),
        };
        
        // Process point...
    } else {
        tracing::warn!("ID {} has no location", record.id);
    }
}
```

### Special Cases

#### IDs without a location
**Possible:** Deactivated or new measurement points

**Suggested handling:**
```rust
if location.is_none() {
    if config.skip_unmapped {
        continue; // Skip record
    } else {
        return Err(anyhow!("ID {} has no location", record.id));
    }
}
```

#### Negative values (absence of data)
**Indicates:** Sensor with no data in that 15-min period

**Handling:**
```rust
if record.intensidad < 0 || record.ocupacion < 0 {
    if config.skip_invalid {
        continue;
    } else {
        return Err(ValidationError::MissingData);
    }
}
```

---

## 4. Required Validations

### Temporal Data Validations (CSV)

```rust
impl TrafficRecord {
    fn validate(&self, config: &ValidationConfig) -> Result<()> {
        // 1. Reasonable timestamp
        if self.timestamp > Utc::now() {
            bail!("Future timestamp");
        }
        
        // 2. Intensity in range
        if self.intensidad > 5000 {  // Reasonable maximum
            bail!("Excessive intensity: {}", self.intensidad);
        }
        
        // 3. Occupancy 0-100%
        if self.ocupacion > 100 {
            bail!("Occupancy > 100%");
        }
        
        // 4. Reasonable speed (M-30 only)
        if self.tipo_elem == "C30" && self.vmed > 150 {
            bail!("Excessive speed: {} km/h", self.vmed);
        }
        
        Ok(())
    }
}
```

### Coordinate Validations (Shapefile)

```rust
impl Location {
    fn validate_madrid_bounds(&self) -> Result<()> {
        // Ranges extracted from the real analysis
        const LAT_MIN: f64 = 40.30;
        const LAT_MAX: f64 = 40.55;
        const LON_MIN: f64 = -3.85;
        const LON_MAX: f64 = -3.55;
        
        if self.latitud < LAT_MIN || self.latitud > LAT_MAX {
            bail!("Latitude outside Madrid: {}", self.latitud);
        }
        
        if self.longitud < LON_MIN || self.longitud > LON_MAX {
            bail!("Longitude outside Madrid: {}", self.longitud);
        }
        
        Ok(())
    }
}
```

---

## 5. Technical Considerations

### Reading a Shapefile in Rust

**Recommended crate:** `shapefile` or `gdal`

```rust
// Option 1: shapefile (pure Rust, simpler)
use shapefile::Reader;

let reader = Reader::from_path("pmed_ubicacion_12-2025.shp")?;

for shape_record in reader.iter_shapes_and_records() {
    let (shape, record) = shape_record?;
    
    let id: i32 = record.get("ID")?;
    let lat: f64 = record.get("LATITUD")?;
    let lon: f64 = record.get("LONGITUD")?;
    let nombre: String = record.get("NOMBRE")?;
    
    // Store in HashMap...
}

// Option 2: Convert to CSV with Python (preprocessing)
// Simpler for Rust, but requires an extra step
```

### Estimated Performance

**Shapefile load (5,024 points):**
- Parsing: ~50-100ms
- HashMap insert: ~10ms
- **Total:** <150ms (one-time load at startup)

**Streaming the historical CSV (11M records):**
- With `csv` crate + serde: ~10-15 sec (no processing)
- HashMap lookup: O(1) × 11M ≈ negligible  
- **Estimated total:** <30 sec to process a full month

---

## 6. Next Steps

### Implementation (Phase 1 - Tasks 3-10)

1. ✅ **Tasks 1-2 COMPLETED:** Research and data analysis
2. **Task 3:** Design `struct TrafficPoint` based on this schema
3. **Task 4:** Implement the validations documented here
4. **Task 5:** CSV parser with the `csv` crate
5. **Task 6:** Shapefile parser (or convert to CSV)
6. **Task 7:** Integration pipeline with streaming
7. **Task 8:** Tests with real data

---

## 7. Appendices

### A. Example of a Complete Integrated Record

```json
{
  "id": 1006,
  "timestamp": "2025-12-01T00:15:00Z",
  "location": {
    "name": "M-30 SECTOR X SENTIDO Y",
    "coords": {
      "lat": 40.448526,
      "lon": -3.725273
    },
    "type": "M30"
  },
  "metrics": {
    "intensidad": 732,
    "ocupacion": 2,
    "velocidad": 66,
    "carga": 0
  },
  "quality": {
    "error_flag": "N",
    "periodo_integracion": 5
  }
}
```

### B. Required Rust Dependencies

```toml
[dependencies]
csv = "1.3"                   # Fast CSV parser
serde = { version = "1.0", features = ["derive"] }
chrono = { version = "0.4", features = ["serde"] }
shapefile = "0.5"             # Shapefile reading
# Or alternative: gdal = "0.16"  (more complete but requires C libs)
```

### C. References

- **Official CSV documentation:** `docs/madridDocs/traficohisotricodesde2013.txt`
- **Official real-time documentation:** `docs/madridDocs/traficotiemporeal.txt`
- **Geodetic system:** EPSG:25830 (ETRS89 UTM Zone 30N)
- **Data portal:** https://datos.madrid.es/portal/site/egob/
