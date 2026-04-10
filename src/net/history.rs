use crate::data::types::KlineBar;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("JSON parse failed: {0}")]
    Parse(String),
    #[error("API returned empty data")]
    Empty,
}

pub async fn fetch_history(
    symbol: &str,
    period: u32,
    max_attempts: usize,
) -> Result<Vec<KlineBar>, Error> {
    fetch_international_history(symbol, period, max_attempts).await
}

async fn fetch_international_history(
    symbol: &str,
    period: u32,
    max_attempts: usize,
) -> Result<Vec<KlineBar>, Error> {
    let code = if let Some(stripped) = symbol.strip_prefix("hf_") {
        stripped.to_string()
    } else {
        symbol.to_uppercase()
    };

    let var_name = format!("_{}_{}_{}", code, period, code.len());
    let url = format!(
        "https://gu.sina.cn/ft/api/jsonp.php/var%20{}%3D/GlobalService.getMink",
        urlencoding::encode(&var_name)
    );

    let params = [("symbol", &code), ("type", &period.to_string())];
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::ACCEPT, "*/*".parse().unwrap());
    headers.insert(
        reqwest::header::REFERER,
        format!("https://finance.sina.com.cn/futures/quotes/{}.shtml", code)
            .parse()
            .unwrap(),
    );
    headers.insert(
        reqwest::header::USER_AGENT,
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36"
            .parse()
            .unwrap(),
    );

    let client = reqwest::Client::new();

    for attempt in 0..max_attempts {
        let response = client
            .get(&url)
            .query(&params)
            .headers(headers.clone())
            .send()
            .await?;

        let text = response.text().await?;

        let raw = extract_json_array(&text)?;

        let wrapped = format!("[{}]", raw);
        let value: serde_json::Value = serde_json::from_str(&wrapped)
            .map_err(|e| Error::Parse(format!("JSON decode: {}", e)))?;

        let data = value
            .as_array()
            .ok_or_else(|| Error::Parse("not an array".to_string()))?;

        if data.is_empty() {
            tracing::warn!("attempt {}: empty data", attempt + 1);
            continue;
        }

        let bars: Vec<KlineBar> = data
            .iter()
            .filter_map(|v| {
                let obj = v.as_object()?;
                let time_str = obj.get("d")?.as_str()?;
                let datetime = parse_datetime(time_str);
                let id = compute_bucket_id(datetime, period);
                let datetime = id * duration_ns(period);
                Some(KlineBar {
                    id,
                    datetime,
                    open: obj.get("o")?.as_str()?.parse().ok()?,
                    high: obj.get("h")?.as_str()?.parse().ok()?,
                    low: obj.get("l")?.as_str()?.parse().ok()?,
                    close: obj.get("c")?.as_str()?.parse().ok()?,
                    volume: obj.get("v")?.as_str()?.parse().ok()?,
                    open_interest: obj.get("p")?.as_str().unwrap_or("0").parse().ok()?,
                })
            })
            .collect();

        if bars.is_empty() {
            tracing::warn!("attempt {}: no valid bars", attempt + 1);
            continue;
        }

        return Ok(bars);
    }

    Err(Error::Empty)
}

fn parse_datetime(time_str: &str) -> i64 {
    use chrono::{NaiveDateTime, TimeZone, Utc};

    let formats = [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y/%m/%d %H:%M:%S",
        "%Y/%m/%d %H:%M",
    ];

    for fmt in &formats {
        if let Ok(dt) = NaiveDateTime::parse_from_str(time_str, fmt) {
            return Utc
                .from_utc_datetime(&dt)
                .timestamp_nanos_opt()
                .unwrap_or(0);
        }
    }

    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(time_str) {
        return dt.timestamp_nanos_opt().unwrap_or(0);
    }

    0
}

fn duration_ns(period: u32) -> i64 {
    (period as i64) * 60 * 1_000_000_000
}

fn compute_bucket_id(datetime_ns: i64, period: u32) -> i64 {
    let d = duration_ns(period);
    if d <= 0 {
        return 0;
    }
    datetime_ns.div_euclid(d)
}

fn extract_json_array(text: &str) -> Result<&str, Error> {
    let start = text
        .find("=([")
        .ok_or_else(|| Error::Parse("no =([ pattern".to_string()))?;

    let array_start = start + 3;
    let mut depth = 1;
    let mut i = array_start;

    while i < text.len() {
        match text.as_bytes()[i] {
            b'[' => depth += 1,
            b']' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            break;
        }
        i += 1;
    }

    if depth != 0 {
        return Err(Error::Parse("unmatched brackets".to_string()));
    }

    Ok(&text[array_start..i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_array_valid() {
        let text = r#"var _sh510050_5_sh510050=(["field1","field2"]);"#;
        let result = extract_json_array(text);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), r#""field1","field2""#);
    }

    #[test]
    fn test_extract_json_array_nested() {
        let text = r#"var test=([{"a":[1,2]},{"b":3}]);"#;
        let result = extract_json_array(text);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), r#"{"a":[1,2]},{"b":3}"#);
    }

    #[test]
    fn test_extract_json_array_no_pattern() {
        let text = "some random text without pattern";
        let result = extract_json_array(text);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_json_array_unmatched_brackets() {
        let text = "var test=([1,2,3);";
        let result = extract_json_array(text);
        assert!(result.is_err());
    }

    #[test]
    fn test_compute_bucket_id_minute_step() {
        let period = 1;
        let dt0 = parse_datetime("2020-01-01 00:00");
        let dt1 = parse_datetime("2020-01-01 00:01");
        let id0 = compute_bucket_id(dt0, period);
        let id1 = compute_bucket_id(dt1, period);
        assert_eq!(id1 - id0, 1);
    }
}
