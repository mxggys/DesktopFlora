//! Local weather from Open-Meteo, and the seasons where someone lives, turned into what falls on the desktop.

use crate::simulation::Mode;

/// How much falls for a light, steady and heavy rain or snowfall, in percent.
pub const LIGHT: u32 = 25;
pub const STEADY: u32 = 50;
pub const HEAVY: u32 = 75;

/// What the sky is doing now, from a forecast's `current` block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Conditions {
    /// Rain or snow and how much, in percent; `None` under a dry sky, when the season decides instead.
    pub falling: Option<(u32, Mode)>,
    /// Scales the breeze, from the wind speed outside.
    pub wind: f32,
}

/// The weather of a season at a latitude: snow in winter, blossoms in spring, stars on summer nights, leaves in autumn.
/// South of the equator the seasons are the other way round; near it, where there are none to speak of, blossoms.
pub fn season(month: u32, latitude: f64) -> Mode {
    if latitude.abs() < 23.5 {
        return Mode::Blossom;
    }
    let month = if latitude < 0.0 { (month + 5) % 12 + 1 } else { month };
    match month {
        3..=5 => Mode::Blossom,
        6..=8 => Mode::Stars,
        9..=11 => Mode::Autumn,
        _ => Mode::Snow,
    }
}

/// The path of the forecast request for a place, rounded to about a kilometre so the request doesn't pinpoint anyone.
pub fn forecast_path(latitude: f64, longitude: f64) -> String {
    format!("/v1/forecast?latitude={latitude:.2}&longitude={longitude:.2}&current=weather_code,wind_speed_10m&wind_speed_unit=kmh")
}

/// Reads Open-Meteo's answer.
pub fn conditions(json: &str) -> Option<Conditions> {
    // `current_units` comes first and names the same keys, so read only past `"current":`.
    let current = &json[json.find("\"current\":")? + 10..];
    let code = number(current, "weather_code")? as u32;
    let wind_speed = number(current, "wind_speed_10m").unwrap_or(12.0) as f32;
    let falling = match code {
        51 | 53 | 55 | 56 | 57 | 61 | 66 | 80 => Some((LIGHT, Mode::Rain)),
        63 | 81 => Some((STEADY, Mode::Rain)),
        65 | 67 | 82 => Some((HEAVY, Mode::Rain)),
        95 => Some((HEAVY, Mode::Storm)),
        96 | 99 => Some((HEAVY, Mode::Hail)),
        45 | 48 => Some((STEADY, Mode::Fog)),
        71 | 77 | 85 => Some((LIGHT, Mode::Snow)),
        73 => Some((STEADY, Mode::Snow)),
        75 | 86 => Some((HEAVY, Mode::Snow)),
        _ => None,
    };
    Some(Conditions { falling, wind: (wind_speed / 12.0).clamp(0.3, 3.0) })
}

fn number(json: &str, key: &str) -> Option<f64> {
    let rest = &json[json.find(&format!("\"{key}\""))? + key.len() + 2..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start();
    let end = rest.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &str = r#"{"latitude":52.52,"current_units":{"time":"iso8601","weather_code":"wmo code","wind_speed_10m":"km/h"},
        "current":{"time":"2026-01-10T12:00","interval":900,"weather_code":73,"wind_speed_10m":24.5}}"#;

    #[test]
    fn reads_current_conditions() {
        assert_eq!(conditions(ANSWER), Some(Conditions { falling: Some((STEADY, Mode::Snow)), wind: 24.5 / 12.0 }));
        let clear = ANSWER.replace("\"weather_code\":73", "\"weather_code\":0");
        assert_eq!(conditions(&clear).map(|c| c.falling), Some(None));
        let storm = ANSWER.replace("\"weather_code\":73", "\"weather_code\":95");
        assert_eq!(conditions(&storm).and_then(|c| c.falling), Some((HEAVY, Mode::Storm)));
        assert_eq!(conditions("{}"), None);
    }

    #[test]
    fn seasons_follow_the_hemisphere() {
        // Berlin and Sydney in November, and in July.
        assert_eq!(season(11, 52.5), Mode::Autumn);
        assert_eq!(season(11, -33.9), Mode::Blossom);
        assert_eq!(season(7, 52.5), Mode::Stars);
        assert_eq!(season(7, -33.9), Mode::Snow);
        assert_eq!(season(1, 52.5), Mode::Snow);
        assert_eq!(season(1, -33.9), Mode::Stars);
        // Singapore has no seasons to speak of.
        assert_eq!(season(1, 1.3), Mode::Blossom);
    }

    #[test]
    fn rounds_the_place() {
        assert_eq!(
            forecast_path(52.520_08, 13.404_95),
            "/v1/forecast?latitude=52.52&longitude=13.40&current=weather_code,wind_speed_10m&wind_speed_unit=kmh"
        );
    }
}
