//! Local weather from Open-Meteo, and the seasons where someone lives, turned into what falls on the desktop.

use crate::simulation::Mode;

/// How much falls for a light, steady and heavy rain or snowfall, in percent, when the service names only its kind.
pub const LIGHT: u32 = 25;
pub const STEADY: u32 = 50;
pub const HEAVY: u32 = 75;

/// What the sky is doing now, from a forecast's `current` block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Conditions {
    /// Rain or snow and how much, in percent; `None` under a dry sky, when the season decides instead.
    pub falling: Option<(u32, Mode)>,
    /// What falls along with it: snow with rain near freezing, as sleet.
    pub also: Option<Mode>,
    /// Scales the breeze, from the wind and its gusts outside.
    pub wind: f32,
    /// The wind outside, in km/h.
    pub wind_speed: f32,
    /// Daylight, rather than night.
    pub day: bool,
    /// How much of the sky is cloud, in percent.
    pub clouds: f32,
    /// The air outside, in °C.
    pub temperature: f32,
    /// How damp the air is, in percent.
    pub humidity: f32,
    /// Rain or snow on its way while it is dry, and in about how many minutes: within the hour, by the quarter.
    pub coming: Option<(Mode, u32)>,
}

impl Conditions {
    /// How far frost and mist cover the glass in this weather, 0 to 1: frost below freezing, more the colder; mist in
    /// cold, damp air, as in rain or fog.
    pub fn glass(&self) -> (f32, f32) {
        let frost = ((1.0 - self.temperature) / 5.0).clamp(0.0, 1.0);
        let damp = matches!(self.falling, Some((_, Mode::Rain | Mode::Storm | Mode::Fog | Mode::Hail))) || self.humidity >= 92.0;
        let mist = if damp && self.temperature <= 10.0 { (0.5 + (10.0 - self.temperature) / 20.0).min(1.0) } else { 0.0 };
        (frost, mist)
    }
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

/// What falls and how much while following the sky outside. Rain, snow, storms, hail and fog fall as they do there.
/// Under a dry sky only a pinch of the season's weather comes: a few leaves in autumn, more of them in the wind,
/// a few petals in spring, a flake or two in winter, and on a clear night the stars, with fireflies on a warm summer
/// one.
pub fn outside(month: u32, latitude: f64, sky: &Conditions) -> (u32, Vec<Mode>) {
    // Rain or snow within the hour, under a dry sky: it starts with a few drops or flakes, among the season's.
    if sky.falling.is_none()
        && let Some((coming, _)) = sky.coming
    {
        let seasonal = season(month, latitude);
        let first = if seasonal == coming || seasonal == Mode::Stars { vec![coming] } else { vec![seasonal, coming] };
        return (8, first);
    }
    if let Some((amount, mode)) = sky.falling {
        let mut modes = Vec::new();
        if let Some(also) = sky.also.filter(|&also| also != mode) {
            modes.push(also);
        }
        modes.push(mode);
        return (amount, modes);
    }
    let windy = ((sky.wind_speed - 8.0) / 30.0).clamp(0.0, 1.0);
    let clear_night = !sky.day && sky.clouds < 45.0;
    let (amount, mode) = match season(month, latitude) {
        Mode::Autumn => (8.0 + 14.0 * windy, Mode::Autumn),
        Mode::Blossom => (7.0 + 8.0 * windy, Mode::Blossom),
        Mode::Snow => (if sky.temperature <= 2.0 { 7.0 } else { 4.0 }, Mode::Snow),
        // Summer: stars and fireflies at night, dandelion seeds floating by day.
        _ if !sky.day && sky.temperature >= 14.0 => return (12, if clear_night { vec![Mode::Fireflies, Mode::Stars] } else { vec![Mode::Fireflies] }),
        _ if !sky.day => (10.0, Mode::Stars),
        _ => (6.0, Mode::Pollen),
    };
    let amount = amount.round() as u32;
    if clear_night && mode != Mode::Stars { (amount, vec![Mode::Stars, mode]) } else { (amount, vec![mode]) }
}

/// How far the moon is through its cycle at a moment, in seconds since 1970: 0 at new moon, 0.5 at full.
pub fn moon_phase(unix_seconds: f64) -> f32 {
    // A new moon on 6 January 2000 at 18:14 UTC, and the mean length of a lunar month.
    const NEW_MOON: f64 = 947_182_440.0;
    const SYNODIC_MONTH: f64 = 29.530_588_853 * 86_400.0;
    ((unix_seconds - NEW_MOON) / SYNODIC_MONTH).rem_euclid(1.0) as f32
}

/// The path of the forecast request for a place, rounded to about a kilometre so the request doesn't pinpoint anyone.
pub fn forecast_path(latitude: f64, longitude: f64) -> String {
    format!(
        "/v1/forecast?latitude={latitude:.2}&longitude={longitude:.2}&current=weather_code,precipitation,rain,showers,snowfall,cloud_cover,is_day,temperature_2m,relative_humidity_2m,wind_speed_10m,wind_gusts_10m&minutely_15=precipitation,snowfall&forecast_minutely_15=5&wind_speed_unit=kmh"
    )
}

/// How much falls for so many millimetres of rain an hour: a drizzle shows a little, a downpour a lot.
fn rain_amount(millimetres: f64) -> u32 {
    (15.0 + 26.0 * (1.0 + millimetres).log2()).clamp(15.0, 90.0) as u32
}

/// How much falls for so many centimetres of snow an hour.
fn snow_amount(centimetres: f64) -> u32 {
    (20.0 + 32.0 * (1.0 + 2.0 * centimetres).log2()).clamp(20.0, 90.0) as u32
}

/// Reads Open-Meteo's answer.
pub fn conditions(json: &str) -> Option<Conditions> {
    // `current_units` comes first and names the same keys, so read only past `"current":`.
    let after = &json[json.find("\"current\":")? + 10..];
    // Only the current block itself, so a key it lacks isn't read out of the next one.
    let current = &after[..after.find('}').map_or(after.len(), |end| end + 1)];
    let code = number(current, "weather_code")? as u32;
    let wind_speed = number(current, "wind_speed_10m").unwrap_or(12.0) as f32;
    let gusts = number(current, "wind_gusts_10m").unwrap_or(wind_speed as f64) as f32;
    let temperature = number(current, "temperature_2m").unwrap_or(10.0) as f32;
    let rain = number(current, "rain").unwrap_or(0.0) + number(current, "showers").unwrap_or(0.0);
    let snow = number(current, "snowfall").unwrap_or(0.0);
    let precipitation = number(current, "precipitation").unwrap_or(rain);
    // How much by the amount measured, where there is one; by the kind of weather named, where there isn't.
    let wet = |named: u32| if rain.max(precipitation) > 0.0 { rain_amount(rain.max(precipitation)) } else { named };
    let snowy = |named: u32| if snow > 0.0 { snow_amount(snow) } else { named };
    let falling = match code {
        51 | 53 | 55 | 56 | 57 | 61 | 66 | 80 => Some((wet(LIGHT), Mode::Rain)),
        63 | 81 => Some((wet(STEADY), Mode::Rain)),
        65 | 67 | 82 => Some((wet(HEAVY), Mode::Rain)),
        95 => Some((wet(HEAVY).max(STEADY), Mode::Storm)),
        96 | 99 => Some((wet(HEAVY).max(STEADY), Mode::Hail)),
        45 | 48 => Some((STEADY, Mode::Fog)),
        71 | 77 | 85 => Some((snowy(LIGHT), Mode::Snow)),
        73 => Some((snowy(STEADY), Mode::Snow)),
        75 | 86 => Some((snowy(HEAVY), Mode::Snow)),
        // Named dry, yet something is measured falling: it falls, by what it is.
        _ if snow > 0.05 => Some((snow_amount(snow), Mode::Snow)),
        _ if rain > 0.1 => Some((rain_amount(rain), Mode::Rain)),
        _ => None,
    };
    // Rain near freezing comes with some snow, and freezing rain is sleet.
    let also = match falling {
        Some((_, Mode::Rain)) if temperature <= 2.0 || snow > 0.0 || matches!(code, 56 | 57 | 66 | 67) => Some(Mode::Snow),
        Some((_, Mode::Snow)) if rain > 0.2 => Some(Mode::Rain),
        _ => None,
    };
    Some(Conditions {
        falling,
        also,
        wind: (wind_speed.max(gusts * 0.6) / 12.0).clamp(0.3, 3.0),
        wind_speed: wind_speed.max(gusts * 0.6),
        day: number(current, "is_day").is_none_or(|day| day != 0.0),
        clouds: number(current, "cloud_cover").unwrap_or(50.0) as f32,
        temperature,
        humidity: number(current, "relative_humidity_2m").unwrap_or(70.0) as f32,
        coming: coming(json, temperature),
    })
}

/// The first quarter of the coming hour with rain or snow in the forecast, and which, if any.
fn coming(json: &str, temperature: f32) -> Option<(Mode, u32)> {
    let block = &json[json.find("\"minutely_15\":")?..];
    let precipitation = series(block, "precipitation")?;
    let snowfall = series(block, "snowfall").unwrap_or_default();
    // The first is the quarter under way now, which the current weather already tells.
    let quarter = (1..precipitation.len().min(5)).find(|&i| precipitation[i] >= 0.1)?;
    let snow = snowfall.get(quarter).is_some_and(|&cm| cm > 0.0) || temperature <= 0.5;
    Some((if snow { Mode::Snow } else { Mode::Rain }, quarter as u32 * 15))
}

/// The numbers in the array under `key`.
fn series(json: &str, key: &str) -> Option<Vec<f64>> {
    let rest = &json[json.find(&format!("\"{key}\""))? + key.len() + 2..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('[')?;
    let end = rest.find(']')?;
    Some(rest[..end].split(',').map(|value| value.trim().parse().unwrap_or(0.0)).collect())
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
        "current":{"time":"2026-01-10T12:00","interval":900,"weather_code":73,"precipitation":0.0,"rain":0.0,"showers":0.0,
        "snowfall":0.0,"cloud_cover":100,"is_day":1,"temperature_2m":-2.0,"relative_humidity_2m":80,"wind_speed_10m":24.5,"wind_gusts_10m":30.0}"#;

    fn sky(code: u32, extra: &[(&str, &str)]) -> Conditions {
        let mut answer = ANSWER.replace("\"weather_code\":73", &format!("\"weather_code\":{code}"));
        for (key, value) in extra {
            let start = answer.rfind(&format!("\"{key}\":")).unwrap() + key.len() + 3;
            let end = start + answer[start..].find([',', '}']).unwrap();
            answer.replace_range(start..end, value);
        }
        conditions(&answer).unwrap()
    }

    #[test]
    fn reads_current_conditions() {
        let snowing = conditions(ANSWER).unwrap();
        assert_eq!(snowing.falling, Some((STEADY, Mode::Snow)));
        assert_eq!(snowing.wind, 24.5 / 12.0);
        assert_eq!(snowing.temperature, -2.0);
        assert!(snowing.day);
        assert_eq!(sky(0, &[]).falling, None);
        assert_eq!(sky(95, &[]).falling, Some((HEAVY, Mode::Storm)));
        assert_eq!(conditions("{}"), None);
    }

    #[test]
    fn falls_by_how_much_is_measured() {
        let drizzle = sky(61, &[("rain", "0.2"), ("precipitation", "0.2"), ("temperature_2m", "9")]).falling.unwrap().0;
        let downpour = sky(65, &[("rain", "6.0"), ("precipitation", "6.0"), ("temperature_2m", "9")]).falling.unwrap().0;
        assert!(drizzle < 30 && downpour > 70, "drizzle {drizzle}, downpour {downpour}");
        let flurry = sky(71, &[("snowfall", "0.1")]).falling.unwrap().0;
        let blizzard = sky(75, &[("snowfall", "3.0")]).falling.unwrap().0;
        assert!(flurry < 35 && blizzard > 80, "flurry {flurry}, blizzard {blizzard}");
        // Called overcast, yet it rains.
        assert_eq!(sky(3, &[("rain", "1.0"), ("temperature_2m", "9")]).falling.map(|f| f.1), Some(Mode::Rain));
    }

    #[test]
    fn rain_near_freezing_brings_snow() {
        let sleet = sky(61, &[("rain", "0.5"), ("temperature_2m", "1.0")]);
        assert_eq!(outside(1, 52.5, &sleet).1, vec![Mode::Snow, Mode::Rain]);
        let warm = sky(61, &[("rain", "0.5"), ("temperature_2m", "8.0")]);
        assert_eq!(outside(1, 52.5, &warm).1, vec![Mode::Rain]);
    }

    #[test]
    fn a_dry_sky_brings_only_a_pinch_of_the_season() {
        let calm = sky(1, &[("wind_speed_10m", "4"), ("wind_gusts_10m", "6"), ("temperature_2m", "10")]);
        let stormy = sky(1, &[("wind_speed_10m", "40"), ("wind_gusts_10m", "70"), ("temperature_2m", "10")]);
        let (few, leaves) = outside(10, 52.5, &calm);
        let (more, _) = outside(10, 52.5, &stormy);
        assert_eq!(leaves, vec![Mode::Autumn]);
        assert!(few <= 10 && more > few * 2, "calm {few}, windy {more}");
        assert_eq!(outside(4, 52.5, &calm).1, vec![Mode::Blossom]);
        assert!(outside(1, 52.5, &calm).0 <= 8);
        // A clear autumn night: the stars come out over the leaves.
        let night = sky(0, &[("is_day", "0"), ("cloud_cover", "10"), ("temperature_2m", "6")]);
        assert_eq!(outside(10, 52.5, &night).1, vec![Mode::Stars, Mode::Autumn]);
        // A warm summer night has fireflies under the stars; a summer day only a few petals.
        let summer_night = sky(0, &[("is_day", "0"), ("cloud_cover", "10"), ("temperature_2m", "19")]);
        assert_eq!(outside(7, 52.5, &summer_night).1, vec![Mode::Fireflies, Mode::Stars]);
        let summer_day = sky(0, &[("temperature_2m", "24")]);
        assert_eq!(outside(7, 52.5, &summer_day), (6, vec![Mode::Pollen]));
    }

    #[test]
    fn rain_on_its_way_starts_with_a_few_drops() {
        let forecast = |values: &str| {
            let answer = ANSWER.replace("\"weather_code\":73", "\"weather_code\":2").replace("\"temperature_2m\":-2.0", "\"temperature_2m\":8.0")
                + &format!(r#","minutely_15_units":{{"precipitation":"mm"}},"minutely_15":{{"time":[],"precipitation":[{values}],"snowfall":[0,0,0,0,0]}}}}"#);
            conditions(&answer).unwrap()
        };
        let dry = forecast("0,0,0,0,0");
        assert_eq!(dry.coming, None);
        let soon = forecast("0,0,0.4,1.2,2.0");
        assert_eq!(soon.coming, Some((Mode::Rain, 30)));
        assert_eq!(outside(10, 52.5, &soon), (8, vec![Mode::Autumn, Mode::Rain]));
        // What falls already is told by the current weather, not by the forecast.
        assert_eq!(forecast("0.5,0,0,0,0").coming, None);
    }

    #[test]
    fn knows_the_moon() {
        // Full moon on 5 November 2025 at 13:19 UTC; new moon on 20 November 2025 at 06:47 UTC.
        assert!((moon_phase(1_762_348_740.0) - 0.5).abs() < 0.03);
        let new = moon_phase(1_763_621_220.0);
        assert!(!(0.03..=0.97).contains(&new), "{new}");
    }

    #[test]
    fn frost_and_mist_on_the_glass() {
        let freezing = sky(0, &[("temperature_2m", "-4")]);
        assert_eq!(freezing.glass().0, 1.0);
        assert_eq!(sky(0, &[("temperature_2m", "6")]).glass(), (0.0, 0.0));
        let cold_rain = sky(63, &[("rain", "1.0"), ("temperature_2m", "5")]);
        assert!(cold_rain.glass().1 > 0.5);
        let warm_rain = sky(63, &[("rain", "1.0"), ("temperature_2m", "20")]);
        assert_eq!(warm_rain.glass().1, 0.0);
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
        let path = forecast_path(52.520_08, 13.404_95);
        assert!(path.starts_with("/v1/forecast?latitude=52.52&longitude=13.40&current="), "{path}");
        assert!(path.contains("precipitation") && path.contains("is_day") && path.contains("wind_gusts_10m"));
    }
}
