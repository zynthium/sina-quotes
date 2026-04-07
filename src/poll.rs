use crate::types::Quote;
use thiserror::Error;
use std::time::Duration;

#[derive(Error, Debug)]
pub enum Error {
    #[error("HTTP request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("parse error: {0}")]
    Parse(String),
}

pub type Result<T> = std::result::Result<T, Error>;

fn build_international_url(codes: &[&str]) -> String {
    let prefixed: Vec<String> = codes.iter().map(|c| {
        if c.starts_with("hf_") {
            c.to_string()
        } else {
            format!("hf_{}", c.to_uppercase())
        }
    }).collect();
    let joined = prefixed.join(",");
    format!("https://hq.sinajs.cn/?list={}", joined)
}

fn parse_international_quote(text: &str) -> Result<Quote> {
    let start = text.find("=\"");
    if start.is_none() {
        return Err(Error::Parse("no =\" pattern".to_string()));
    }
    
    let start = start.unwrap() + 2;
    let end = text.rfind('"').unwrap_or(text.len());
    let data = &text[start..end];
    let data = data.trim_end_matches(';');
    
    if data.is_empty() {
        return Err(Error::Parse("empty data".to_string()));
    }
    
    let fields: Vec<&str> = data.split(',').collect();
    if fields.len() < 10 {
        return Err(Error::Parse(format!("insufficient fields: {}", fields.len())));
    }
    
    let symbol = fields.first().unwrap_or(&"unknown").to_string();
    let idx_price = if fields.get(1).map(|s| s.is_empty()).unwrap_or(true) { 8 } else { 1 };
    let price: f64 = fields.get(idx_price).unwrap_or(&"0").parse().unwrap_or(0.0);
    let volume: f64 = fields.get(9).unwrap_or(&"0").parse().unwrap_or(0.0);
    
    Ok(Quote {
        symbol,
        price,
        bid_price: 0.0,
        ask_price: 0.0,
        open: 0.0,
        high: 0.0,
        low: 0.0,
        prev_settle: 0.0,
        settle_price: 0.0,
        volume,
        quote_time: String::new(),
        date: String::new(),
        name: String::new(),
        timestamp: chrono::Utc::now().timestamp(),
    })
}

pub async fn poll_international(
    symbols: &[&str],
    interval_secs: u64,
) -> Result<impl futures_util::Stream<Item = std::result::Result<Quote, Error>>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| Error::Parse(e.to_string()))?;

    let url = build_international_url(symbols);
    
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::ACCEPT, "*/*".parse().unwrap());
    headers.insert(reqwest::header::REFERER, "https://finance.sina.com.cn/".parse().unwrap());
    headers.insert(reqwest::header::USER_AGENT, "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36".parse().unwrap());
    headers.insert("Sec-Fetch-Dest", "script".parse().unwrap());
    headers.insert("Sec-Fetch-Mode", "no-cors".parse().unwrap());
    headers.insert("Sec-Fetch-Site", "cross-site".parse().unwrap());

    let (tx, rx) = tokio::sync::mpsc::channel(100);

    tokio::spawn(async move {
        loop {
            match client.get(&url).headers(headers.clone()).send().await {
                Ok(response) => {
                    match response.text().await {
                        Ok(text) => {
                            if let Ok(quote) = parse_international_quote(&text) {
                                let _ = tx.send(Ok(quote)).await;
                            }
                        }
                        Err(e) => {
                            tracing::warn!("fetch error: {}", e);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("request error: {}", e);
                }
            }
            
            tokio::time::sleep(Duration::from_secs(interval_secs)).await;
        }
    });

    Ok(tokio_stream::wrappers::ReceiverStream::new(rx))
}
