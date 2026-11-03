//! Weather through Open-Meteo (free, no key, non-commercial; blueprint §4.8): the forecast at a place,
//! and a place's coordinates from its name. Only these two addresses.

use serde_json::{Value, json};

use crate::plain::plain;

const MOST_PLACES: usize = 5;
const LONGEST_NAME: usize = 80;
const FORECAST: &str = "https://api.open-meteo.com/v1/forecast";
const GEOCODING: &str = "https://geocoding-api.open-meteo.com/v1/search";

/// A latitude or longitude within `limit` of zero, as sent; else why not.
pub fn coordinate(text: &str, limit: f64) -> Result<&str, String> {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || c == '-' || c == '.');
    text.parse::<f64>()
        .ok()
        .filter(|n| plain && n.abs() <= limit)
        .map(|_| text)
        .ok_or_else(|| format!("{text:?} is not a coordinate"))
}

/// Open-Meteo's answer as the little Kestrel uses.
pub fn parse_forecast(body: &Value) -> Result<Value, String> {
    let daily = &body["daily"];
    let day = |i: usize| {
        json!({
            "date": daily["time"][i],
            "code": daily["weather_code"][i],
            "max": daily["temperature_2m_max"][i],
            "min": daily["temperature_2m_min"][i],
            "rain": daily["precipitation_probability_max"][i],
        })
    };
    let now = &body["current"];
    if !now["temperature_2m"].is_number() || !daily["time"][0].is_string() {
        return Err("the weather service gave no forecast".into());
    }
    Ok(json!({
        "now": {"temp": now["temperature_2m"], "code": now["weather_code"]},
        "today": day(0),
        "tomorrow": day(1),
    }))
}

/// Its places for a name.
pub fn parse_places(body: &Value) -> Value {
    let found = body["results"].as_array().map_or(&[][..], Vec::as_slice);
    let places: Vec<Value> = found
        .iter()
        .take(MOST_PLACES)
        .filter_map(|p| {
            let name = plain(p["name"].as_str()?, LONGEST_NAME);
            let (lat, lon) = (p["latitude"].as_f64()?, p["longitude"].as_f64()?);
            let parts: Vec<String> = [&p["admin1"], &p["country"]]
                .iter()
                .filter_map(|v| v.as_str())
                .map(|t| plain(t, LONGEST_NAME))
                .collect();
            Some(json!({
                "name": name,
                "where": parts.join(", "),
                "lat": lat.to_string(),
                "lon": lon.to_string(),
            }))
        })
        .collect();
    json!({"places": places})
}

pub fn weather(lat: &str, lon: &str) -> Result<Value, String> {
    let (lat, lon) = (coordinate(lat, 90.0)?, coordinate(lon, 180.0)?);
    crate::fetch::check(FORECAST)?;
    let (status, body) = crate::fetch::get_json(
        FORECAST,
        &[
            ("latitude", lat),
            ("longitude", lon),
            ("current", "temperature_2m,weather_code"),
            (
                "daily",
                "weather_code,temperature_2m_max,temperature_2m_min,precipitation_probability_max",
            ),
            ("timezone", "auto"),
            ("forecast_days", "2"),
        ],
    )?;
    if status != 200 {
        return Err(format!("the weather service said {status}"));
    }
    parse_forecast(&body)
}

pub fn place(name: &str) -> Result<Value, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > LONGEST_NAME {
        return Err("a place's name, please".into());
    }
    crate::fetch::check(GEOCODING)?;
    let (status, body) = crate::fetch::get_json(
        GEOCODING,
        &[
            ("name", name),
            ("count", "5"),
            ("language", "en"),
            ("format", "json"),
        ],
    )?;
    if status != 200 {
        return Err(format!("the weather service said {status}"));
    }
    Ok(parse_places(&body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_numbers_in_range_are_coordinates() {
        for good in ["26.46", "-33.9", "0", "90"] {
            assert_eq!(coordinate(good, 90.0).unwrap(), good);
        }
        for bad in ["91", "-90.5", "abc", "", "1;2", "NaN", "inf", "1&x=2"] {
            assert!(coordinate(bad, 90.0).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_forecast_comes_as_now_today_and_tomorrow() {
        let body = json!({
            "timezone": "Asia/Kolkata",
            "current": {"temperature_2m": 28.4, "precipitation": 0.2, "weather_code": 61},
            "daily": {
                "time": ["2026-11-04", "2026-11-05"],
                "weather_code": [63, 2],
                "temperature_2m_max": [31.0, 33.5],
                "temperature_2m_min": [19.5, 20.0],
                "precipitation_probability_max": [80, 10]
            }
        });
        let v = parse_forecast(&body).unwrap();
        assert_eq!(v["now"]["temp"], 28.4);
        assert_eq!(v["now"]["code"], 61);
        assert_eq!(v["today"]["date"], "2026-11-04");
        assert_eq!(v["today"]["max"], 31.0);
        assert_eq!(v["today"]["min"], 19.5);
        assert_eq!(v["today"]["rain"], 80);
        assert_eq!(v["today"]["code"], 63);
        assert_eq!(v["tomorrow"]["date"], "2026-11-05");
        assert_eq!(v["tomorrow"]["rain"], 10);
    }

    #[test]
    fn an_answer_without_a_forecast_is_an_error() {
        assert!(parse_forecast(&json!({"error": true, "reason": "no"})).is_err());
        assert!(parse_forecast(&json!({"current": {}})).is_err());
    }

    #[test]
    fn places_come_with_where_they_are() {
        let body = json!({"results": [
            {"name": "Kanpur", "admin1": "Uttar Pradesh", "country": "India", "latitude": 26.46, "longitude": 80.32, "extra": 1},
            {"name": "Kanpur", "country": "Somewhere", "latitude": 1.0, "longitude": 2.0}
        ]});
        let v = parse_places(&body);
        assert_eq!(v["places"][0]["name"], "Kanpur");
        assert_eq!(v["places"][0]["where"], "Uttar Pradesh, India");
        assert_eq!(v["places"][0]["lat"], "26.46");
        assert_eq!(v["places"][0]["lon"], "80.32");
        assert_eq!(v["places"][1]["where"], "Somewhere");
        assert_eq!(parse_places(&json!({}))["places"], json!([]));
    }
}
