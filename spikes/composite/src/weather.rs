//! Optional, in-memory weather for Sky. No location discovery and no disk cache.
//!
//! Source: MET Norway Locationforecast 2.0 (forecast model output, not observations).
//! https://api.met.no/doc/License — CC BY 4.0; the sky is a procedural adaptation.
//! https://api.met.no/doc/TermsOfService — identify the app, respect Expires and
//! Last-Modified, and arrange a caching gateway before exceeding 20 requests/s
//! across installations. No API key, account, or new HTTP runtime is required.
//!
//! `sample` only polls a bounded worker; it never performs network/file IO on
//! the UI thread. Call it only for a visible Sky. Opt-out/clearing Place must
//! call `revoke`; in-flight curl is cancelled and its result cannot repopulate
//! the cache. A single chosen location and at most one worker are retained.

use serde_json::Value;
use std::io::Read;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex, OnceLock,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_BODY: usize = 512 * 1024;
const MAX_HEADERS: u64 = 16 * 1024;
const MAX_SAMPLES: usize = 256;
const STALE_AFTER: u64 = 3600;
const SOURCE_TOO_OLD: u64 = 24 * 3600;

/// Wind direction is meteorological: the direction it comes FROM, clockwise
/// from north. `height_m` is above ground for surface wind; unavailable aloft.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Wind {
    pub speed_mps: Option<f32>,
    pub direction_deg: Option<f32>,
    pub height_m: Option<f32>,
}

/// Provider symbol, without pretending MET's codes are WMO numeric codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeatherCode {
    bytes: [u8; 64],
    len: u8,
}
impl WeatherCode {
    fn new(s: &str) -> Option<Self> {
        if s.is_empty() || s.len() > 64 || !s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
            return None;
        }
        let mut bytes = [0; 64];
        bytes[..s.len()].copy_from_slice(s.as_bytes());
        Some(Self {
            bytes,
            len: s.len() as u8,
        })
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len as usize]).unwrap_or("")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WeatherSnapshot {
    /// Cloud and humidity fractions in 0..=1. Missing is unknown, never zero.
    pub cloud_cover: Option<f32>,
    pub cloud_low: Option<f32>,
    pub cloud_mid: Option<f32>,
    pub cloud_high: Option<f32>,
    pub humidity: Option<f32>,
    pub fog: Option<f32>,
    pub visibility_m: Option<f32>,
    /// Accumulated forecast precipitation for the NEXT hour; not an observation.
    pub precipitation_mm: Option<f32>,
    pub weather_code: Option<WeatherCode>,
    pub wind_10m: Wind,
    pub wind_80m: Wind,
    pub wind_850hpa: Wind,
    pub wind_500hpa: Wind,
    pub wind_250hpa: Wind,
    /// Forecast validity, provider model update, body fetch, and revalidation.
    pub source_time: u64,
    pub provider_updated_at: Option<u64>,
    pub fetched_at: u64,
    pub checked_at: u64,
    pub stale: bool,
    pub offline: bool,
}
impl WeatherSnapshot {
    pub const SOURCE: &'static str = "MET Norway";
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PlaceKey(i32, i32);
impl PlaceKey {
    fn new(place: Option<(f32, f32)>) -> Option<Self> {
        let (lat, lon) = place?;
        if !lat.is_finite()
            || !lon.is_finite()
            || !(-90.0..=90.0).contains(&lat)
            || !(-180.0..=180.0).contains(&lon)
        {
            return None;
        }
        // About 100m latitude resolution; also avoids cache fragmentation and
        // obeys MET's maximum four decimal places. This is disclosed in Place.
        Some(Self(
            (lat as f64 * 1000.0).round() as i32,
            (lon as f64 * 1000.0).round() as i32,
        ))
    }
    fn url(self) -> String {
        format!(
            "https://api.met.no/weatherapi/locationforecast/2.0/complete?lat={:.3}&lon={:.3}",
            self.0 as f64 / 1000.0,
            self.1 as f64 / 1000.0
        )
    }
}

#[derive(Default)]
struct Forecast {
    samples: Vec<WeatherSnapshot>,
    fetched_at: u64,
    checked_at: u64,
}
impl Forecast {
    fn sample(&self, now: u64, offline: bool) -> Option<WeatherSnapshot> {
        let at = self.samples.partition_point(|s| s.source_time <= now);
        let mut s = *self.samples.get(at.saturating_sub(1))?;
        // Never present a distant future forecast as present conditions.
        if s.source_time > now.saturating_add(3600) {
            return None;
        }
        s.fetched_at = self.fetched_at;
        s.checked_at = self.checked_at;
        s.offline = offline;
        s.stale = offline
            || now.saturating_sub(self.checked_at) > STALE_AFTER
            || now.saturating_sub(s.source_time) > 90 * 60
            || s.provider_updated_at
                .is_some_and(|t| now.saturating_sub(t) > SOURCE_TOO_OLD);
        Some(s)
    }
}

#[derive(Default)]
struct Headers {
    status: u16,
    expires: u64,
    last_modified: Option<String>,
    retry_after: u64,
}
enum Reply {
    Modified(Forecast, Headers),
    Unchanged(Headers),
}
struct Pending {
    key: PlaceKey,
    cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<Reply, Headers>>,
}
#[derive(Default)]
struct Cache {
    key: Option<PlaceKey>,
    forecast: Option<Forecast>,
    pending: Option<Pending>,
    last_modified: Option<String>,
    next_try: Option<Instant>,
    server_not_before: u64,
    offline: bool,
}
impl Cache {
    fn select(&mut self, key: Option<PlaceKey>) {
        if key == self.key {
            return;
        }
        if let Some(p) = &self.pending {
            p.cancel.store(true, Ordering::Release);
        }
        self.key = key;
        self.forecast = None;
        self.last_modified = None;
        self.next_try = None;
        self.server_not_before = 0;
        self.offline = false;
    }
    fn poll(&mut self, now: u64) {
        let Some(p) = &self.pending else {
            return;
        };
        let result = match p.rx.try_recv() {
            Ok(v) => v,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err(Headers::default()),
        };
        let p = self.pending.take().unwrap();
        if p.cancel.load(Ordering::Acquire) || self.key != Some(p.key) {
            return;
        }
        self.next_try = Some(Instant::now() + refresh_interval());
        match result {
            Ok(Reply::Modified(forecast, headers)) => {
                self.forecast = Some(forecast);
                self.offline = false;
                self.server_not_before = headers.expires;
                self.last_modified = headers.last_modified;
            }
            Ok(Reply::Unchanged(headers)) if self.forecast.is_some() => {
                self.forecast.as_mut().unwrap().checked_at = now;
                self.offline = false;
                self.server_not_before = headers.expires;
                if headers.last_modified.is_some() {
                    self.last_modified = headers.last_modified;
                }
            }
            Ok(Reply::Unchanged(_)) => {
                self.offline = true;
                self.last_modified = None;
            }
            Err(headers) => {
                self.offline = true;
                self.server_not_before = headers.retry_after.max(headers.expires);
                if headers.status == 429 || headers.status == 403 {
                    self.next_try = Some(Instant::now() + Duration::from_secs(3600));
                }
            }
        }
    }
    fn due(&self, now: u64) -> bool {
        self.key.is_some()
            && self.pending.is_none()
            && now >= self.server_not_before
            && self.next_try.is_none_or(|t| Instant::now() >= t)
    }
    fn start(&mut self) {
        let Some(key) = self.key else {
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        let modified = self.last_modified.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        match std::thread::Builder::new()
            .name("nus-weather".into())
            .spawn(move || {
                let answer = fetch(key, modified.as_deref(), &token);
                if !token.load(Ordering::Acquire) {
                    let _ = tx.send(answer);
                }
            }) {
            Ok(_) => self.pending = Some(Pending { key, cancel, rx }),
            Err(_) => {
                self.offline = true;
                self.next_try = Some(Instant::now() + refresh_interval());
            }
        }
    }
}

static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
fn cache() -> &'static Mutex<Cache> {
    CACHE.get_or_init(|| Mutex::new(Cache::default()))
}
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn refresh_interval() -> Duration {
    let mut bytes = [0; 2];
    let _ = getrandom::fill(&mut bytes);
    Duration::from_secs(20 * 60 + u16::from_ne_bytes(bytes) as u64 % 301)
}

/// Nonblocking UI entry point. No fetch occurs without both explicit consent
/// and a valid chosen location. Last good data remain available but marked stale
/// after a failed refresh. No timer/worker is kept alive between fetches.
pub fn sample(enabled: bool, place: Option<(f32, f32)>) -> Option<WeatherSnapshot> {
    if !enabled {
        revoke();
        return None;
    }
    let key = PlaceKey::new(place);
    let mut cache = cache().try_lock().ok()?;
    cache.select(key);
    let now = now_unix();
    cache.poll(now);
    if cache.due(now) {
        cache.start();
    }
    cache.forecast.as_ref()?.sample(now, cache.offline)
}

/// Revoke consent immediately, discard all retained location/weather data, and
/// ask the sole in-flight child to stop. An already-sent request cannot be unsent.
pub fn revoke() {
    if let Some(cache) = CACHE.get() {
        if let Ok(mut cache) = cache.lock() {
            cache.select(None);
            cache.poll(now_unix());
        }
    }
}

fn number(v: &Value, units: &Value, name: &str, unit: &str, min: f64, max: f64) -> Option<f32> {
    if units.get(name)?.as_str()? != unit {
        return None;
    }
    let n = v.get(name)?.as_f64()?;
    (n.is_finite() && (min..=max).contains(&n)).then_some(n as f32)
}

fn parse(body: &[u8], fetched_at: u64) -> Result<Forecast, ()> {
    if body.len() > MAX_BODY {
        return Err(());
    }
    let json: Value = serde_json::from_slice(body).map_err(|_| ())?;
    let properties = json.get("properties").ok_or(())?;
    let meta = properties.get("meta").ok_or(())?;
    let units = meta.get("units").ok_or(())?;
    let updated = meta
        .get("updated_at")
        .and_then(Value::as_str)
        .and_then(utc_time);
    let times = properties
        .get("timeseries")
        .and_then(Value::as_array)
        .ok_or(())?;
    if times.len() > MAX_SAMPLES {
        return Err(());
    }
    let mut samples = Vec::with_capacity(times.len());
    for row in times {
        let Some(time) = row.get("time").and_then(Value::as_str).and_then(utc_time) else {
            continue;
        };
        let Some(data) = row.get("data") else {
            continue;
        };
        let Some(details) = data.pointer("/instant/details") else {
            continue;
        };
        let fraction = |name| number(details, units, name, "%", 0.0, 100.0).map(|v| v * 0.01);
        let wind = Wind {
            speed_mps: number(details, units, "wind_speed", "m/s", 0.0, 200.0),
            direction_deg: number(details, units, "wind_from_direction", "degrees", 0.0, 360.0)
                .map(|v| v % 360.0),
            height_m: Some(10.0),
        };
        let hour = data.get("next_1_hours");
        let precipitation_mm = hour
            .and_then(|h| h.get("details"))
            .and_then(|d| number(d, units, "precipitation_amount", "mm", 0.0, 1000.0));
        let weather_code = hour
            .and_then(|h| h.pointer("/summary/symbol_code"))
            .and_then(Value::as_str)
            .and_then(WeatherCode::new);
        samples.push(WeatherSnapshot {
            cloud_cover: fraction("cloud_area_fraction"),
            cloud_low: fraction("cloud_area_fraction_low"),
            cloud_mid: fraction("cloud_area_fraction_medium"),
            cloud_high: fraction("cloud_area_fraction_high"),
            humidity: fraction("relative_humidity"),
            fog: fraction("fog_area_fraction"),
            wind_10m: wind,
            precipitation_mm,
            weather_code,
            source_time: time,
            provider_updated_at: updated,
            fetched_at,
            checked_at: fetched_at,
            ..Default::default()
        });
    }
    samples.sort_by_key(|s| s.source_time);
    samples.dedup_by_key(|s| s.source_time);
    if !samples.iter().any(|s| {
        s.cloud_cover.is_some()
            || s.cloud_low.is_some()
            || s.cloud_mid.is_some()
            || s.cloud_high.is_some()
            || s.humidity.is_some()
            || s.fog.is_some()
            || s.wind_10m.speed_mps.is_some()
            || s.precipitation_mm.is_some()
            || s.weather_code.is_some()
    }) {
        return Err(());
    }
    Ok(Forecast {
        samples,
        fetched_at,
        checked_at: fetched_at,
    })
}

/// MET emits UTC RFC3339 seconds. Reject invalid clocks/dates before using the
/// existing civil-date conversion, which intentionally accepts loose note dates.
fn utc_time(s: &str) -> Option<u64> {
    if s.len() != 20
        || !s.is_ascii()
        || &s[10..11] != "T"
        || &s[13..14] != ":"
        || &s[16..17] != ":"
        || !s.ends_with('Z')
    {
        return None;
    }
    let year = s[0..4].parse::<u32>().ok()?;
    let month = s[5..7].parse::<usize>().ok()?;
    let day = s[8..10].parse::<u32>().ok()?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || day == 0
        || day > days[month - 1]
        || s[11..13].parse::<u32>().ok()? > 23
        || s[14..16].parse::<u32>().ok()? > 59
        || s[17..19].parse::<u32>().ok()? > 59
    {
        return None;
    }
    crate::notes_model::parse_time(s)
}

fn http_time(s: &str) -> Option<u64> {
    let parts: Vec<_> = s.split_ascii_whitespace().collect();
    if parts.len() != 6 || parts[5] != "GMT" {
        return None;
    }
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| *m == parts[2])?
        + 1;
    let day = parts[1].parse::<u8>().ok()?;
    utc_time(&format!("{}-{month:02}-{day:02}T{}Z", parts[3], parts[4]))
}
fn parse_headers(bytes: &[u8], now: u64) -> Headers {
    let mut h = Headers::default();
    for line in String::from_utf8_lossy(bytes).lines() {
        if line.starts_with("HTTP/") {
            h = Headers::default();
            h.status = line
                .split_ascii_whitespace()
                .nth(1)
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("expires") {
            h.expires = http_time(value).unwrap_or(0);
        }
        if name.eq_ignore_ascii_case("last-modified")
            && value.len() < 80
            && http_time(value).is_some()
        {
            h.last_modified = Some(value.to_owned());
        }
        if name.eq_ignore_ascii_case("retry-after") {
            h.retry_after = value
                .parse::<u64>()
                .ok()
                .map(|s| now.saturating_add(s))
                .or_else(|| http_time(value))
                .unwrap_or(0);
        }
    }
    h
}

fn fetch(key: PlaceKey, modified: Option<&str>, cancel: &AtomicBool) -> Result<Reply, Headers> {
    if cancel.load(Ordering::Acquire) {
        return Err(Headers::default());
    }
    let headers_file = tempfile::NamedTempFile::new().map_err(|_| Headers::default())?;
    let mut cmd = crate::updates::curl();
    cmd.args([
        "--connect-timeout",
        "5",
        "--max-time",
        "12",
        "--max-redirs",
        "3",
        "--compressed",
        "--max-filesize",
        "524288",
        "--user-agent",
        "nus/0.0.2 (https://github.com/cbassuarez/nus)",
        "--dump-header",
    ])
    .arg(headers_file.path())
    .arg("--header")
    .arg("Accept: application/json");
    if let Some(value) = modified {
        cmd.arg("--header")
            .arg(format!("If-Modified-Since: {value}"));
    }
    cmd.arg(key.url())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    if cancel.load(Ordering::Acquire) {
        return Err(Headers::default());
    }
    let mut child = cmd.spawn().map_err(|_| Headers::default())?;
    let stdout = child.stdout.take().ok_or_else(Headers::default)?;
    // One short-lived bounded reader lets the worker terminate curl promptly
    // on opt-out, even if a server stalls in the middle of a response body.
    let (tx, rx) = mpsc::sync_channel(1);
    let reader = std::thread::Builder::new()
        .name("nus-weather-body".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            let result = stdout.take((MAX_BODY + 1) as u64).read_to_end(&mut bytes);
            let _ = tx.send(if result.is_ok() && bytes.len() <= MAX_BODY {
                Some(bytes)
            } else {
                None
            });
        });
    let Ok(reader) = reader else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(Headers::default());
    };
    let started = Instant::now();
    let mut body = None;
    let success = loop {
        match rx.try_recv() {
            Ok(value) => {
                body = Some(value);
            }
            Err(mpsc::TryRecvError::Disconnected) if body.is_none() => body = Some(None),
            _ => {}
        }
        if cancel.load(Ordering::Acquire)
            || started.elapsed() > Duration::from_secs(13)
            || body == Some(None)
            || headers_file
                .as_file()
                .metadata()
                .is_ok_and(|m| m.len() > MAX_HEADERS)
        {
            let _ = child.kill();
            let _ = child.wait();
            break false;
        }
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
        }
    };
    let _ = reader.join();
    if body.is_none() {
        body = rx.try_recv().ok();
    }
    let now = now_unix();
    let mut header_bytes = Vec::new();
    let _ = headers_file
        .as_file()
        .take(MAX_HEADERS + 1)
        .read_to_end(&mut header_bytes);
    if header_bytes.len() > MAX_HEADERS as usize || cancel.load(Ordering::Acquire) {
        return Err(Headers::default());
    }
    let headers = parse_headers(&header_bytes, now);
    if !success {
        return Err(headers);
    }
    match headers.status {
        304 => Ok(Reply::Unchanged(headers)),
        200 | 203 => {
            let Some(Some(body)) = body else {
                return Err(headers);
            };
            let Ok(forecast) = parse(&body, now) else {
                return Err(headers);
            };
            Ok(Reply::Modified(forecast, headers))
        }
        _ => Err(headers),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        serde_json::json!({"properties":{"meta":{"updated_at":"2026-09-29T12:05:00Z","units":{"cloud_area_fraction":"%","cloud_area_fraction_low":"%","cloud_area_fraction_medium":"%","cloud_area_fraction_high":"%","relative_humidity":"%","wind_speed":"m/s","wind_from_direction":"degrees","precipitation_amount":"mm"}},"timeseries":[{"time":"2026-09-29T12:00:00Z","data":{"instant":{"details":{"cloud_area_fraction":75,"cloud_area_fraction_low":60,"cloud_area_fraction_medium":20,"cloud_area_fraction_high":15,"relative_humidity":72,"wind_speed":4.5,"wind_from_direction":270}},"next_1_hours":{"summary":{"symbol_code":"lightrain_day"},"details":{"precipitation_amount":0.7}}}},{"time":"2026-09-29T13:00:00Z","data":{"instant":{"details":{"cloud_area_fraction":25}}}}]}})
    }
    fn parsed(value: &Value) -> Forecast {
        parse(
            &serde_json::to_vec(value).unwrap(),
            utc_time("2026-09-29T12:10:00Z").unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn parses_provider_units_and_preserves_unknown_fields() {
        let f = parsed(&fixture());
        let s = f.sample(f.fetched_at, false).unwrap();
        assert_eq!(s.cloud_cover, Some(0.75));
        assert!((s.cloud_low.unwrap() - 0.6).abs() < 0.000_001);
        assert_eq!(s.wind_10m.speed_mps, Some(4.5));
        assert_eq!(s.wind_10m.direction_deg, Some(270.0));
        assert_eq!(s.weather_code.unwrap().as_str(), "lightrain_day");
        assert_eq!(s.visibility_m, None);
        assert_eq!(s.wind_850hpa, Wind::default());
        assert!(!s.stale);
        assert_eq!(s.precipitation_mm, Some(0.7));
    }
    #[test]
    fn missing_null_wrong_units_and_ranges_stay_unknown() {
        let mut v = fixture();
        let d = &mut v["properties"]["timeseries"][0]["data"]["instant"]["details"];
        d["cloud_area_fraction_low"] = Value::Null;
        d["cloud_area_fraction_medium"] = serde_json::json!(101);
        d["cloud_area_fraction_high"] = serde_json::json!(-1);
        d["wind_speed"] = serde_json::json!(500);
        v["properties"]["meta"]["units"]["relative_humidity"] = serde_json::json!("fraction");
        let s = parsed(&v).samples[0];
        assert!(s.cloud_low.is_none() && s.cloud_mid.is_none() && s.cloud_high.is_none());
        assert!(s.wind_10m.speed_mps.is_none() && s.humidity.is_none());
    }
    #[test]
    fn cached_series_advances_hour_and_marks_offline_or_old() {
        let mut f = parsed(&fixture());
        let later = utc_time("2026-09-29T13:10:00Z").unwrap();
        f.checked_at = later;
        let s = f.sample(later, false).unwrap();
        assert_eq!(s.cloud_cover, Some(0.25));
        assert_eq!(s.precipitation_mm, None);
        assert!(!s.stale);
        assert!(f.sample(later, true).unwrap().stale);
        assert!(f.sample(later + 7200, false).unwrap().stale);
    }
    #[test]
    fn location_keys_are_bounded_coarse_and_do_not_alias_distinct_places() {
        assert!(PlaceKey::new(None).is_none());
        assert!(PlaceKey::new(Some((91.0, 0.0))).is_none());
        assert!(PlaceKey::new(Some((f32::NAN, 0.0))).is_none());
        assert_eq!(
            PlaceKey::new(Some((10.0001, 20.0001))),
            PlaceKey::new(Some((10.0002, 20.0002)))
        );
        assert_ne!(
            PlaceKey::new(Some((10.0, 20.0))),
            PlaceKey::new(Some((10.0, -20.0)))
        );
        assert!(PlaceKey::new(Some((10.1234, -20.5678)))
            .unwrap()
            .url()
            .ends_with("lat=10.123&lon=-20.568"));
    }
    #[test]
    fn optout_cancels_pending_and_rejects_late_success() {
        let key = PlaceKey(10000, 20000);
        let token = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(1);
        let mut c = Cache {
            key: Some(key),
            forecast: Some(parsed(&fixture())),
            pending: Some(Pending {
                key,
                cancel: token.clone(),
                rx,
            }),
            ..Default::default()
        };
        c.select(None);
        assert!(token.load(Ordering::Acquire));
        assert!(c.forecast.is_none());
        assert!(!c.due(now_unix()));
        tx.send(Ok(Reply::Modified(parsed(&fixture()), Headers::default())))
            .ok();
        c.poll(now_unix());
        assert!(c.forecast.is_none());
        assert!(c.key.is_none());
        assert!(c.pending.is_none());
    }
    #[test]
    fn location_change_does_not_leak_old_result_or_create_parallel_work() {
        let key = PlaceKey(10000, 20000);
        let token = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(1);
        let mut c = Cache {
            key: Some(key),
            forecast: Some(parsed(&fixture())),
            pending: Some(Pending {
                key,
                cancel: token,
                rx,
            }),
            ..Default::default()
        };
        c.select(Some(PlaceKey(20000, 30000)));
        assert!(!c.due(now_unix()));
        assert!(c.forecast.is_none());
        tx.send(Ok(Reply::Modified(parsed(&fixture()), Headers::default())))
            .ok();
        c.poll(now_unix());
        assert!(c.forecast.is_none());
        assert!(c.due(now_unix()));
    }
    #[test]
    fn cache_headers_respect_final_response_expiry_and_conditional_token() {
        let h = parse_headers(b"HTTP/1.1 301 Moved\r\nExpires: Tue, 29 Sep 2026 15:00:00 GMT\r\n\r\nHTTP/2 304\r\nExpires: Tue, 29 Sep 2026 12:30:00 GMT\r\nLast-Modified: Tue, 29 Sep 2026 12:05:00 GMT\r\n", 0);
        assert_eq!(h.status, 304);
        assert_eq!(h.expires, utc_time("2026-09-29T12:30:00Z").unwrap());
        assert!(h.last_modified.is_some());
        assert_eq!(
            parse_headers(b"HTTP/2 429\r\nRetry-After: 7200\r\n", 100).retry_after,
            7300
        );
    }
    #[test]
    fn failed_refresh_keeps_good_data_and_honors_server_backoff() {
        let key = PlaceKey(10000, 20000);
        let (tx, rx) = mpsc::sync_channel(1);
        let mut c = Cache {
            key: Some(key),
            forecast: Some(parsed(&fixture())),
            pending: Some(Pending {
                key,
                cancel: Arc::new(AtomicBool::new(false)),
                rx,
            }),
            ..Default::default()
        };
        let now = utc_time("2026-09-29T12:20:00Z").unwrap();
        tx.send(Err(Headers {
            status: 429,
            retry_after: now + 7200,
            ..Default::default()
        }))
        .ok();
        c.poll(now);
        assert!(
            c.forecast
                .as_ref()
                .unwrap()
                .sample(now, c.offline)
                .unwrap()
                .stale
        );
        assert!(!c.due(now + 7199));
        assert_eq!(c.server_not_before, now + 7200);
    }
    #[test]
    fn conditional_success_retains_forecast_and_refreshes_validation_time() {
        let key = PlaceKey(10000, 20000);
        let (tx, rx) = mpsc::sync_channel(1);
        let mut c = Cache {
            key: Some(key),
            forecast: Some(parsed(&fixture())),
            offline: true,
            last_modified: Some("Tue, 29 Sep 2026 12:05:00 GMT".into()),
            pending: Some(Pending {
                key,
                cancel: Arc::new(AtomicBool::new(false)),
                rx,
            }),
            ..Default::default()
        };
        let now = utc_time("2026-09-29T13:10:00Z").unwrap();
        tx.send(Ok(Reply::Unchanged(Headers {
            status: 304,
            expires: now + 3600,
            ..Default::default()
        })))
        .ok();
        c.poll(now);
        let s = c.forecast.as_ref().unwrap().sample(now, c.offline).unwrap();
        assert_eq!(s.cloud_cover, Some(0.25));
        assert!(!s.stale);
        assert_eq!(s.checked_at, now);
        assert_ne!(s.fetched_at, s.checked_at);
        assert!(c.last_modified.is_some());
        assert!(!c.due(now));
    }
    #[test]
    fn bounded_parser_rejects_invalid_dates_and_oversized_data() {
        assert!(utc_time("2026-02-30T12:00:00Z").is_none());
        assert!(utc_time("2026-09-29T24:00:00Z").is_none());
        assert!(utc_time("2026-09-29T12:00:00+01:00").is_none());
        assert!(utc_time("2024-02-29T12:00:00Z").is_some());
        assert!(parse(&vec![b' '; MAX_BODY + 1], 0).is_err());
        assert!(parse(b"{\"error\":true}", 0).is_err());
        assert!(WeatherCode::new("<unexpected>").is_none());
        let mut empty = fixture();
        empty["properties"]["timeseries"] =
            serde_json::json!([{"time":"2026-09-29T12:00:00Z","data":{"instant":{"details":{}}}}]);
        assert!(parse(&serde_json::to_vec(&empty).unwrap(), 0).is_err());
    }
}
