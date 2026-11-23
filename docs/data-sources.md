# Data Sources - Madrid Traffic

## Main Dataset: Traffic History (2013-2026)

**Source:** Open Data Portal of the Ayuntamiento de Madrid (Madrid City Council)  
**URL:** https://datos.madrid.es/portal/site/egob/  
**Dataset:** "Tráfico: Histórico de datos del tráfico desde 2013" (Traffic: historical traffic data since 2013)  
**License:** Open Data (academic use permitted)  
**Updates:** Monthly (first half of the month)

### Technical Characteristics
- **Format:** Compressed CSV (.zip) - Delimiter: `;`
- **Volume:** ~11 million records/month (~765 MB uncompressed)
- **Granularity:** 15 minutes
- **Historical period:** Since 2013
- **Measurement points:** ~4,100 active in Madrid
  - ~3,800 urban area
  - ~300 M-30

### Sensor Infrastructure
- 7,800 detectors in total
- Technology: Electromagnetic loops (professional-grade accuracy)
- Standardization: AEN/CTN 199 "Equipamiento para la Gestión del Tráfico" (Traffic Management Equipment)
- Geodetic system: ETRS89 (RD 1071/2007)

### Available Fields (Historical CSV)

**Downloaded:** December 2025 (`12-2025.csv`)

| Field | Type | Description | Example |
|-------|------|-------------|---------|
| `id` | Integer | Unique identifier of the measurement point | 1006 |
| `fecha` | String | Timestamp `YYYY-MM-DD HH:MI:SS` | 2025-12-01 00:15:00 |
| `tipo_elem` | String | Type: `Urbano` (urban) or `C30` (M-30) | C30 |
| `intensidad` | Integer | Vehicles/hour | 732 |
| `ocupacion` | Integer | % of time the detector is occupied | 2 |
| `carga` | Integer | Congestion level (0-100) | 0 |
| `vmed` | Integer | Average speed km/h (M-30 only) | 66 |
| `error` | String | Quality flag: N/E/S | N |
| `periodo_integracion` | Integer | Samples received (1-5) | 5 |

**⚠️ Note:** The historical CSV does NOT contain GPS coordinates. See the complementary file below.

---

## Complementary Dataset: Location of Measurement Points

**URL:** https://datos.madrid.es/portal/site/egob/  
**Dataset:** "Tráfico: Ubicación de los puntos de medida" (Traffic: location of measurement points)  
**Format:** Shapefile (ESRI GIS format)  
**License:** Open Data

### Technical Characteristics
- **Format:** Shapefile (.shp + auxiliary files)
- **Coordinate system:** EPSG:25830 (ETRS89 / UTM zone 30N)
- **Geometry:** Polygons (detection area)
- **Records:** 5,024 measurement points
- **Updated:** Monthly (matches the historical data)

### Available Fields (Shapefile)

**Downloaded:** December 2025 (`pmed_ubicacion_12-2025.shp`)

| Field | Type | Description | Example |
|-------|------|-------------|---------|
| `ID` | Integer | **JOIN field with the historical CSV** | 10011 |
| `NOMBRE` | String | Description of the point (street + direction) | ANTONIO NOVAIS O-E... |
| `TIPO_ELEM` | String | URB / M30 / other | URB |
| `UTM_X` | Float | UTM X coordinate (meters) | 438497.626 |
| `UTM_Y` | Float | UTM Y coordinate (meters) | 4477793.706 |
| **`LATITUD`** | **Float** | **WGS84 latitude** ✅ | **40.448526** |
| **`LONGITUD`** | **Float** | **WGS84 longitude** ✅ | **-3.725273** |
| `DISTRITO` | Integer | District code (1-21) | 15 |
| `COD_CENT` | String | Central controller code | 240102 |
| `geometry` | Geometry | GIS polygon of the detection area | POLYGON(...) |

**✅ Key advantage:** WGS84 coordinates (lat/lon) are already converted, no transformation required.

### Point Distribution

```
Total: 5,024 points

By type:
  URB (Urban):      4,585 (91.3%)
  M30 (Highway):      314 ( 6.2%)
  other:              125 ( 2.5%)
```

### Geographic Coverage

**Area:** Madrid metropolitan region

**WGS84 ranges:**
- Latitude: 40.33°N - 40.52°N (~24 km)
- Longitude: -3.84°W - -3.55°W (~20 km)

---

## Source Integration

### Relationship between Datasets

```
┌─────────────────────────────┐
│  Historical CSV             │
│  11M records/month          │
│  Field: id                  │
└──────────┬──────────────────┘
           │
           │ JOIN ON id=ID
           ▼
┌─────────────────────────────┐
│  Locations Shapefile        │
│  5,024 unique points        │
│  Field: ID + coords         │
└─────────────────────────────┘
```

**Relationship:** 1 location point → N temporal records (one every 15 min)

**Implementation strategy:**
1. Load the shapefile into a HashMap<ID, Location> (one-time load)
2. Stream the historical CSV line by line
3. O(1) lookup of coordinates by ID
4. Discard records whose ID is not found

**Technical details:** See `docs/gps-data-schema.md`

---

## Data Quality

### Sample Analysis (1,000 rows)

✅ **Null values:** 0  
✅ **Error flags:** 100% normal (N)  
✅ **Coordinates:** 100% within the Madrid range  
✅ **Timestamps:** Consistent format  

**Conclusion:** High-quality data, professionally validated by the Ayuntamiento de Madrid.

---

## References

- **Official documentation:**
  - Historical CSV: `docs/madridDocs/traficohisotricodesde2013.txt`
  - Real time: `docs/madridDocs/traficotiemporeal.txt`
- **Detailed schema:** `docs/gps-data-schema.md`
- **Portal:** https://datos.madrid.es/portal/site/egob/
- **Standards:** AEN/CTN 199, RD 1071/2007
