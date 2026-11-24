//! The weather where the computer is: Windows locates it, if allowed, and Open-Meteo says what the sky is doing there.
//! Without a location, the seasons follow the country Windows is set to.

use windows::Devices::Geolocation::{GeolocationAccessStatus, Geolocator, PositionAccuracy};
use windows::Foundation::TimeSpan;

/// A second, in the hundred-nanosecond ticks of a `TimeSpan`.
const SECOND: i64 = 10_000_000;
use windows::Win32::Globalization::{GEO_FRIENDLYNAME, GEO_LATITUDE, GEOCLASS_NATION, GetGeoInfoW, GetUserGeoID};

use snowhush_core::weather::{Conditions, conditions, forecast_path};

use crate::update::fetch;

/// What is falling outside now, and the latitude it was asked for. Errors explain what to do, as turning on location access.
pub fn now() -> Result<(Conditions, f64), String> {
    // A place picked by name comes first; else where Windows says the computer is.
    let (latitude, longitude) = match crate::prefs::place() {
        Some((_, latitude, longitude)) => (latitude, longitude),
        None => locate()?,
    };
    let body = fetch("api.open-meteo.com", &forecast_path(latitude, longitude), "application/json")?;
    let conditions =
        conditions(&String::from_utf8_lossy(&body)).ok_or_else(|| "the weather service gave an answer DesktopFlora doesn't understand".to_string())?;
    Ok((conditions, latitude))
}

/// Looks a place up by name with Open-Meteo's place search: its name as it is known, with its region or country,
/// and where it lies.
pub fn find_place(name: &str) -> Result<(String, f64, f64), String> {
    let language = crate::text::t("en");
    let language = if language.len() == 2 { language } else { "en".to_string() };
    let path = format!("/v1/search?name={}&count=1&language={language}&format=json", encode(name.trim()));
    let body = fetch("geocoding-api.open-meteo.com", &path, "application/json")?;
    let json = String::from_utf8_lossy(&body);
    let not_found = || crate::text::t("No place of that name was found. Try the town's name alone.");
    let result = &json[json.find("\"results\"").ok_or_else(not_found)?..];
    let latitude = number(result, "latitude").ok_or_else(not_found)?;
    let longitude = number(result, "longitude").ok_or_else(not_found)?;
    let found = text(result, "name").unwrap_or_else(|| name.trim().to_string());
    let region = text(result, "admin1").or_else(|| text(result, "country"));
    let label = match region {
        Some(region) if region != found => format!("{found}, {region}"),
        _ => found,
    };
    Ok((label, latitude, longitude))
}

/// A name made safe to put in a web address.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) { (byte as char).to_string() } else { format!("%{byte:02X}") })
        .collect()
}

fn number(json: &str, key: &str) -> Option<f64> {
    let rest = &json[json.find(&format!("\"{key}\""))? + key.len() + 2..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

fn text(json: &str, key: &str) -> Option<String> {
    let rest = &json[json.find(&format!("\"{key}\""))? + key.len() + 2..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_string())
}

/// The country set in Windows' region settings, by name in the person's language, and its latitude, for the seasons;
/// central Europe if it can't be read.
pub fn home() -> (String, f64) {
    unsafe {
        let nation = GetUserGeoID(GEOCLASS_NATION);
        let info = |kind| {
            let mut buffer = [0u16; 128];
            let length = GetGeoInfoW(nation, kind, Some(&mut buffer), 0);
            (length > 1).then(|| String::from_utf16_lossy(&buffer[..length as usize - 1]).trim().to_string())
        };
        let latitude = info(GEO_LATITUDE).and_then(|text| text.replace(',', ".").parse().ok()).unwrap_or(50.0);
        (info(GEO_FRIENDLYNAME).unwrap_or_default(), latitude)
    }
}

fn locate() -> Result<(f64, f64), String> {
    let denied = || {
        crate::text::t(
            "Windows didn't share the location. Turn on Location and \"Let desktop apps access your location\" in Settings → Privacy & security → Location.",
        )
    };
    let access = Geolocator::RequestAccessAsync().and_then(|operation| operation.join()).map_err(|error| error.message())?;
    if access != GeolocationAccessStatus::Allowed {
        return Err(denied());
    }
    // A fresh fix from Wi-Fi or GPS, no older than ten minutes, finds the street rather than the region an internet
    // address points to; if none comes within fifteen seconds, whatever Windows already knows will do.
    let fix = |accuracy: PositionAccuracy, age: i64, timeout: i64| {
        let locator = Geolocator::new()?;
        locator.SetDesiredAccuracy(accuracy)?;
        locator
            .GetGeopositionAsyncWithAgeAndTimeout(TimeSpan { Duration: age * SECOND }, TimeSpan { Duration: timeout * SECOND })?
            .join()?
            .Coordinate()?
            .Point()?
            .Position()
    };
    let position = fix(PositionAccuracy::High, 10 * 60, 15).or_else(|_| fix(PositionAccuracy::Default, 60 * 60, 30)).map_err(|_| denied())?;
    Ok((position.Latitude, position.Longitude))
}
