use crate::data::types::{FuturesCategory, FuturesMarketHours, KlineBar, TradingSession};
use serde::Deserialize;
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

#[derive(Debug, Deserialize)]
struct MarketHoursPayload {
    #[serde(default)]
    timezone: String,
    #[serde(default)]
    exchange: String,
    #[serde(default)]
    interval: String,
    #[serde(default, rename = "time")]
    sessions: Vec<[String; 2]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InferredMarketSymbol {
    pub(crate) category: FuturesCategory,
    pub(crate) code: String,
}

pub async fn fetch_history(
    symbol: &str,
    duration_seconds: u32,
    max_attempts: usize,
) -> Result<Vec<KlineBar>, Error> {
    fetch_international_history(symbol, duration_seconds, max_attempts).await
}

pub async fn fetch_market_hours(
    symbol: &str,
    max_attempts: usize,
) -> Result<FuturesMarketHours, Error> {
    let inferred = infer_market_symbol(symbol);
    let category = inferred.category;
    let code = inferred.code;
    let var_name = format!("kke_future_{}_{}", category.as_str(), code);
    let url = format!(
        "https://stock.finance.sina.com.cn/futures/api/jsonp.php/var%20{}=/InterfaceInfoService.getMarket",
        urlencoding::encode(&var_name)
    );

    let params = [("category", category.as_str()), ("symbol", code.as_str())];
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
        let market = parse_market_hours_jsonp(category, &code, &text)?;

        if market.sessions.is_empty() {
            tracing::warn!("attempt {}: empty market sessions", attempt + 1);
            continue;
        }

        return Ok(market);
    }

    Err(Error::Empty)
}

async fn fetch_international_history(
    symbol: &str,
    duration_seconds: u32,
    max_attempts: usize,
) -> Result<Vec<KlineBar>, Error> {
    if duration_seconds == 0 {
        return Err(Error::Parse("duration_seconds must be > 0".to_string()));
    }

    if duration_seconds >= 86_400 {
        let daily = fetch_international_daily_history(symbol, max_attempts).await?;
        let bars = aggregate_daily_bars(daily, duration_seconds)?;
        if bars.is_empty() {
            return Err(Error::Empty);
        }
        return Ok(bars);
    }

    if !duration_seconds.is_multiple_of(60) {
        return Err(Error::Parse(
            "intraday duration_seconds must be a multiple of 60".to_string(),
        ));
    }

    let period_minutes = duration_seconds / 60;
    fetch_international_mink_history(symbol, period_minutes, max_attempts).await
}

async fn fetch_international_mink_history(
    symbol: &str,
    period_minutes: u32,
    max_attempts: usize,
) -> Result<Vec<KlineBar>, Error> {
    let code = if let Some(stripped) = symbol.strip_prefix("hf_") {
        stripped.to_string()
    } else {
        symbol.to_uppercase()
    };

    let var_name = format!("_{}_{}_{}", code, period_minutes, code.len());
    let url = format!(
        "https://gu.sina.cn/ft/api/jsonp.php/var%20{}%3D/GlobalService.getMink",
        urlencoding::encode(&var_name)
    );

    let params = [("symbol", &code), ("type", &period_minutes.to_string())];
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
    let duration_seconds = period_minutes.saturating_mul(60);

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
                let id = compute_bucket_id(datetime, duration_seconds);
                let datetime = id * duration_ns(duration_seconds);
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

async fn fetch_international_daily_history(
    symbol: &str,
    max_attempts: usize,
) -> Result<Vec<KlineBar>, Error> {
    use chrono::Datelike;

    let code = if let Some(stripped) = symbol.strip_prefix("hf_") {
        stripped.to_string()
    } else {
        symbol.to_uppercase()
    };

    let now = chrono::Utc::now().date_naive();
    let cache_buster = format!("{}_{}_{}", now.year(), now.month(), now.day());
    let var_name = format!("_{}{}", code, cache_buster);

    let url = format!(
        "https://stock2.finance.sina.com.cn/futures/api/jsonp.php/var%20{}=/GlobalFuturesService.getGlobalFuturesDailyKLine",
        urlencoding::encode(&var_name)
    );

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
    let duration_seconds = 86_400u32;

    for attempt in 0..max_attempts {
        let response = client
            .get(&url)
            .query(&[
                ("symbol", code.as_str()),
                ("_", cache_buster.as_str()),
                ("source", "web"),
            ])
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

        let mut bars: Vec<KlineBar> = data
            .iter()
            .filter_map(|v| {
                let obj = v.as_object()?;
                let date_str = obj.get("date")?.as_str()?;
                let datetime = parse_datetime(date_str);
                let id = compute_bucket_id(datetime, duration_seconds);
                let datetime = id * duration_ns(duration_seconds);
                Some(KlineBar {
                    id,
                    datetime,
                    open: obj.get("open")?.as_str()?.parse().ok()?,
                    high: obj.get("high")?.as_str()?.parse().ok()?,
                    low: obj.get("low")?.as_str()?.parse().ok()?,
                    close: obj.get("close")?.as_str()?.parse().ok()?,
                    volume: obj.get("volume")?.as_str()?.parse().ok()?,
                    open_interest: obj
                        .get("position")
                        .and_then(|v| v.as_str())
                        .unwrap_or("0")
                        .parse()
                        .ok()?,
                })
            })
            .collect();

        if bars.is_empty() {
            tracing::warn!("attempt {}: no valid bars", attempt + 1);
            continue;
        }

        bars.sort_by_key(|b| b.id);
        return Ok(bars);
    }

    Err(Error::Empty)
}

fn aggregate_daily_bars(
    bars: Vec<KlineBar>,
    duration_seconds: u32,
) -> Result<Vec<KlineBar>, Error> {
    if duration_seconds < 86_400 || !duration_seconds.is_multiple_of(86_400) {
        return Err(Error::Parse(
            "duration_seconds must be a multiple of 86400 for daily-or-higher periods".to_string(),
        ));
    }

    if duration_seconds == 86_400 {
        let mut bars = bars;
        bars.sort_by_key(|b| b.id);
        bars.dedup_by_key(|b| b.id);
        return Ok(bars);
    }

    use std::collections::BTreeMap;

    let duration_days = (duration_seconds / 86_400) as usize;
    let mut buckets: BTreeMap<i64, BTreeMap<i64, KlineBar>> = BTreeMap::new();

    for bar in bars {
        let bucket_id = compute_bucket_id(bar.datetime, duration_seconds);
        let day_id = compute_bucket_id(bar.datetime, 86_400);
        buckets.entry(bucket_id).or_default().insert(day_id, bar);
    }

    let bars = buckets
        .into_iter()
        .filter_map(|(id, day_bars)| {
            if day_bars.len() != duration_days {
                return None;
            }

            let mut iter = day_bars.into_values();
            let first = iter.next()?;

            let mut high = first.high;
            let mut low = first.low;
            let mut close = first.close;
            let mut volume = first.volume;
            let mut open_interest = first.open_interest;

            for day_bar in iter {
                if day_bar.high > high {
                    high = day_bar.high;
                }
                if day_bar.low < low {
                    low = day_bar.low;
                }
                close = day_bar.close;
                volume += day_bar.volume;
                open_interest = day_bar.open_interest;
            }

            Some(KlineBar {
                id,
                datetime: id * duration_ns(duration_seconds),
                open: first.open,
                high,
                low,
                close,
                volume,
                open_interest,
            })
        })
        .collect();

    Ok(bars)
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

    let date_formats = ["%Y-%m-%d", "%Y/%m/%d"];
    for fmt in &date_formats {
        if let Ok(d) = chrono::NaiveDate::parse_from_str(time_str, fmt)
            && let Some(dt) = d.and_hms_opt(0, 0, 0)
        {
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

fn duration_ns(duration_seconds: u32) -> i64 {
    (duration_seconds as i64) * 1_000_000_000
}

fn compute_bucket_id(datetime_ns: i64, duration_seconds: u32) -> i64 {
    let d = duration_ns(duration_seconds);
    if d <= 0 {
        return 0;
    }
    datetime_ns.div_euclid(d)
}

pub(crate) fn infer_market_symbol(symbol: &str) -> InferredMarketSymbol {
    let trimmed = symbol.trim();
    if let Some(stripped) = trimmed.strip_prefix("hf_") {
        return InferredMarketSymbol {
            category: FuturesCategory::Hf,
            code: stripped.to_ascii_uppercase(),
        };
    }

    if let Some(stripped) = trimmed.strip_prefix("nf_") {
        return InferredMarketSymbol {
            category: FuturesCategory::Nf,
            code: stripped.to_ascii_uppercase(),
        };
    }

    InferredMarketSymbol {
        category: FuturesCategory::Nf,
        code: trimmed.to_ascii_uppercase(),
    }
}

pub(crate) fn market_hours_cache_key(symbol: &str) -> String {
    let inferred = infer_market_symbol(symbol);
    format!("{}:{}", inferred.category.as_str(), inferred.code)
}

fn parse_market_hours_jsonp(
    category: FuturesCategory,
    symbol: &str,
    text: &str,
) -> Result<FuturesMarketHours, Error> {
    let raw = extract_json_object(text)?;
    let payload: MarketHoursPayload =
        serde_json::from_str(raw).map_err(|e| Error::Parse(format!("JSON decode: {}", e)))?;

    let sessions = payload
        .sessions
        .into_iter()
        .map(|[start, end]| TradingSession { start, end })
        .collect();

    Ok(FuturesMarketHours {
        category,
        symbol: symbol.to_string(),
        timezone: payload.timezone,
        exchange: empty_to_none(payload.exchange),
        interval: empty_to_none(payload.interval),
        sessions,
    })
}

fn empty_to_none(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
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

fn extract_json_object(text: &str) -> Result<&str, Error> {
    let start = text
        .find("=({")
        .ok_or_else(|| Error::Parse("no =({ pattern".to_string()))?;

    let object_start = start + 2;
    let mut depth = 1;
    let mut i = object_start + 1;

    while i < text.len() {
        match text.as_bytes()[i] {
            b'{' => depth += 1,
            b'}' => depth -= 1,
            _ => {}
        }
        if depth == 0 {
            return Ok(&text[object_start..i + 1]);
        }
        i += 1;
    }

    Err(Error::Parse("unmatched braces".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::FuturesCategory;

    fn make_bar(
        date: &str,
        open: f64,
        high: f64,
        low: f64,
        close: f64,
        volume: f64,
        open_interest: f64,
    ) -> KlineBar {
        let datetime = parse_datetime(date);
        let id = compute_bucket_id(datetime, 86_400);
        KlineBar {
            id,
            datetime: id * duration_ns(86_400),
            open,
            high,
            low,
            close,
            volume,
            open_interest,
        }
    }

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
    fn test_extract_json_object_valid() {
        let text = r#"var kke_future_hf_FEF=({"timezone":"summer","time":[["07:25","23:59"],["00:00","05:15"]]});"#;
        let result = extract_json_object(text);
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            r#"{"timezone":"summer","time":[["07:25","23:59"],["00:00","05:15"]]}"#
        );
    }

    #[test]
    fn test_infer_market_symbol_hf_prefixed() {
        let inferred = infer_market_symbol("hf_FEF");
        assert_eq!(inferred.category, FuturesCategory::Hf);
        assert_eq!(inferred.code, "FEF");
    }

    #[test]
    fn test_infer_market_symbol_nf_prefixed() {
        let inferred = infer_market_symbol("nf_SC0");
        assert_eq!(inferred.category, FuturesCategory::Nf);
        assert_eq!(inferred.code, "SC0");
    }

    #[test]
    fn test_infer_market_symbol_bare_defaults_to_nf() {
        let inferred = infer_market_symbol("sc0");
        assert_eq!(inferred.category, FuturesCategory::Nf);
        assert_eq!(inferred.code, "SC0");
    }

    #[test]
    fn test_parse_market_hours_hf_jsonp() {
        let text = r#"/*<script>location.href='//sina.com';</script>*/
var kke_future_hf_FEF=({"timezone":"summer","time":[["07:25","23:59"],["00:00","05:15"]],"exchange":"(SGX)","t1url":"http:\/\/stock2.finance.sina.com.cn\/futures\/api\/json.php\/GlobalFuturesService.getGlobalFuturesMinLine?symbol=FEF","t4url":"http:\/\/stock2.finance.sina.com.cn\/futures\/api\/json.php\/GlobalFuturesService.getGlobalFutures5MLine?symbol=FEF","kurl":"http:\/\/stock2.finance.sina.com.cn\/futures\/api\/json.php\/GlobalFuturesService.getGlobalFuturesDailyKLine?symbol=FEF"});"#;

        let market = parse_market_hours_jsonp(FuturesCategory::Hf, "FEF", text).unwrap();

        assert_eq!(market.category, FuturesCategory::Hf);
        assert_eq!(market.symbol, "FEF");
        assert_eq!(market.timezone, "summer");
        assert_eq!(market.exchange.as_deref(), Some("(SGX)"));
        assert_eq!(market.sessions.len(), 2);
        assert_eq!(market.sessions[0].start, "07:25");
        assert_eq!(market.sessions[0].end, "23:59");
        assert_eq!(market.sessions[1].start, "00:00");
        assert_eq!(market.sessions[1].end, "05:15");
    }

    #[test]
    fn test_parse_market_hours_nf_jsonp() {
        let text = r#"/*<script>location.href='//sina.com';</script>*/
var kke_future_nf_SC0=({"interval":"","exchange":"","timezone":"summer","t1url":"http:\/\/stock2.finance.sina.com.cn\/futures\/api\/json.php\/InnerFuturesService.getInnerFuturesMinLine?symbol=SC","t4url":"http:\/\/stock2.finance.sina.com.cn\/futures\/api\/json.php\/InnerFuturesService.getInnerFutures5MLine?symbol=SC","kurl":"http:\/\/stock2.finance.sina.com.cn\/futures\/api\/json.php\/InnerFuturesService.getInnerFuturesDailyKLine?symbol=SC","time":[["21:00","24:00"],["00:00","02:30"],["09:00","11:30"],["13:30","15:00"]]});"#;

        let market = parse_market_hours_jsonp(FuturesCategory::Nf, "SC0", text).unwrap();

        assert_eq!(market.category, FuturesCategory::Nf);
        assert_eq!(market.symbol, "SC0");
        assert_eq!(market.timezone, "summer");
        assert_eq!(market.exchange, None);
        assert_eq!(market.sessions.len(), 4);
        assert_eq!(market.sessions[0].start, "21:00");
        assert_eq!(market.sessions[0].end, "24:00");
        assert_eq!(market.sessions[3].start, "13:30");
        assert_eq!(market.sessions[3].end, "15:00");
    }

    #[test]
    fn test_compute_bucket_id_minute_step() {
        let duration_seconds = 60;
        let dt0 = parse_datetime("2020-01-01 00:00");
        let dt1 = parse_datetime("2020-01-01 00:01");
        let id0 = compute_bucket_id(dt0, duration_seconds);
        let id1 = compute_bucket_id(dt1, duration_seconds);
        assert_eq!(id1 - id0, 1);
    }

    #[test]
    fn test_aggregate_daily_bars_daily_keeps_sorted_unique_days() {
        let bars = vec![
            make_bar("2020-01-02", 2.0, 4.0, 1.5, 3.0, 20.0, 110.0),
            make_bar("2020-01-01", 1.0, 3.0, 0.5, 2.0, 10.0, 100.0),
            make_bar("2020-01-02", 9.0, 9.0, 9.0, 9.0, 99.0, 999.0),
        ];

        let aggregated = aggregate_daily_bars(bars, 86_400).unwrap();

        assert_eq!(aggregated.len(), 2);
        assert_eq!(aggregated[0].datetime, parse_datetime("2020-01-01"));
        assert_eq!(aggregated[1].datetime, parse_datetime("2020-01-02"));
        assert_eq!(aggregated[1].open, 2.0);
    }

    #[test]
    fn test_aggregate_daily_bars_two_days_combines_complete_bucket() {
        let bars = vec![
            make_bar("2020-01-01", 1.0, 3.0, 0.5, 2.0, 10.0, 100.0),
            make_bar("2020-01-02", 2.0, 4.0, 1.5, 3.0, 20.0, 110.0),
        ];

        let aggregated = aggregate_daily_bars(bars, 172_800).unwrap();

        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].open, 1.0);
        assert_eq!(aggregated[0].high, 4.0);
        assert_eq!(aggregated[0].low, 0.5);
        assert_eq!(aggregated[0].close, 3.0);
        assert_eq!(aggregated[0].volume, 30.0);
        assert_eq!(aggregated[0].open_interest, 110.0);
    }

    #[test]
    fn test_aggregate_daily_bars_two_days_skips_incomplete_bucket() {
        let bars = vec![make_bar("2020-01-01", 1.0, 3.0, 0.5, 2.0, 10.0, 100.0)];

        let aggregated = aggregate_daily_bars(bars, 172_800).unwrap();

        assert!(aggregated.is_empty());
    }

    #[test]
    fn test_aggregate_daily_bars_seven_days_combines_full_natural_window() {
        let bars = vec![
            make_bar("2020-01-02", 1.0, 3.0, 0.5, 2.0, 10.0, 100.0),
            make_bar("2020-01-03", 2.0, 4.0, 1.5, 3.0, 20.0, 110.0),
            make_bar("2020-01-04", 3.0, 5.0, 2.5, 4.0, 30.0, 120.0),
            make_bar("2020-01-05", 4.0, 6.0, 3.5, 5.0, 40.0, 130.0),
            make_bar("2020-01-06", 5.0, 7.0, 4.5, 6.0, 50.0, 140.0),
            make_bar("2020-01-07", 6.0, 8.0, 5.5, 7.0, 60.0, 150.0),
            make_bar("2020-01-08", 7.0, 9.0, 6.5, 8.0, 70.0, 160.0),
        ];

        let aggregated = aggregate_daily_bars(bars, 604_800).unwrap();

        assert_eq!(aggregated.len(), 1);
        assert_eq!(aggregated[0].open, 1.0);
        assert_eq!(aggregated[0].high, 9.0);
        assert_eq!(aggregated[0].low, 0.5);
        assert_eq!(aggregated[0].close, 8.0);
        assert_eq!(aggregated[0].volume, 280.0);
        assert_eq!(aggregated[0].open_interest, 160.0);
    }

    #[test]
    fn test_aggregate_daily_bars_rejects_non_daily_multiple() {
        let err = aggregate_daily_bars(Vec::new(), 90_000).unwrap_err();

        assert!(matches!(err, Error::Parse(_)));
    }
}
