use crate::data::types::Quote;
use fastwebsockets::handshake;
use fastwebsockets::{Frame, OpCode, Payload};
use http::Method;
use http::header::{CONNECTION, UPGRADE};
use hyper::Request;
use hyper::body::Bytes;
use std::future::Future;
use thiserror::Error;
use url::Url;

#[derive(Error, Debug)]
#[allow(clippy::result_large_err)]
pub enum Error {
    #[error("WebSocket connection failed: {0}")]
    Connect(String),
    #[error("channel closed")]
    ChannelClosed,
    #[error("parse error: {0}")]
    Parse(String),
    #[error("HTTP error: {0}")]
    Http(#[from] hyper::Error),
    #[error("WebSocket error: {0}")]
    WebSocket(String),
}

pub type Result<T> = std::result::Result<T, Error>;

fn parse_quote_lines(
    text: &str,
    parse_fn: fn(&str) -> std::result::Result<Quote, Error>,
) -> Vec<std::result::Result<Quote, Error>> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(parse_fn)
        .collect()
}

fn normalize_code(code: &str) -> String {
    let code = code.trim();
    if code.is_empty() {
        return String::new();
    }
    if code.contains('.') {
        let parts: Vec<&str> = code.split('.').collect();
        if parts.len() == 2 {
            return format!("{}{}", parts[1], parts[0]).to_lowercase();
        }
    }
    code.to_lowercase()
}

fn build_ws_url(codes: &[&str]) -> String {
    let normalized: Vec<String> = codes.iter().map(|c| normalize_code(c)).collect();
    let joined = normalized.join(",");
    format!("ws://w.sinajs.cn/wskt?list={}", joined)
}

fn build_international_ws_url(codes: &[&str]) -> String {
    let prefixed: Vec<String> = codes
        .iter()
        .map(|c| {
            if c.starts_with("hf_") {
                c.to_string()
            } else {
                format!("hf_{}", c.to_uppercase())
            }
        })
        .collect();
    let joined = prefixed.join(",");
    format!("ws://w.sinajs.cn/wskt?list={}", joined)
}

struct TokioExecutor;

impl<Fut> hyper::rt::Executor<Fut> for TokioExecutor
where
    Fut: Future + Send + 'static,
    Fut::Output: Send + 'static,
{
    fn execute(&self, fut: Fut) {
        tokio::task::spawn(fut);
    }
}

/// 连接 WebSocket 并返回接收流
///
/// `parse_fn` 负责将原始文本解析为 `Quote`，调用方决定解析策略。
async fn connect_and_stream(
    url: String,
    parse_fn: fn(&str) -> std::result::Result<Quote, Error>,
) -> Result<impl futures_util::Stream<Item = std::result::Result<Quote, Error>>> {
    tracing::info!("connecting to {}", url);

    let url_parsed = Url::parse(&url).map_err(|e| Error::Connect(e.to_string()))?;
    let host = url_parsed
        .host_str()
        .ok_or_else(|| Error::Connect("no host".to_string()))?;
    let port = url_parsed.port().unwrap_or(80);
    let path = url_parsed.path();
    let query = url_parsed.query().unwrap_or("");
    let path_with_query = if query.is_empty() {
        path.to_string()
    } else {
        format!("{}?{}", path, query)
    };
    let addr = format!("{}:{}", host, port);

    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::net::TcpStream::connect(&addr),
    )
    .await
    .map_err(|_| Error::Connect("connection timeout".to_string()))?
    .map_err(|e| Error::Connect(e.to_string()))?;

    let req = Request::builder()
        .method(Method::GET)
        .uri(&path_with_query)
        .header("Host", "w.sinajs.cn")
        .header(UPGRADE, "websocket")
        .header(CONNECTION, "upgrade")
        .header("Sec-WebSocket-Key", fastwebsockets::handshake::generate_key())
        .header("Sec-WebSocket-Version", "13")
        .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/146.0.0.0 Safari/537.36")
        .header("Origin", "https://gu.sina.cn")
        .body(http_body_util::Empty::<Bytes>::new())
        .map_err(|e| Error::Connect(e.to_string()))?;

    let (ws, _) = handshake::client(&TokioExecutor, req, stream)
        .await
        .map_err(|e| Error::Connect(e.to_string()))?;

    let mut ws = ws;

    ws.write_frame(Frame::new(
        true,
        OpCode::Text,
        None,
        Payload::Borrowed(b" "),
    ))
    .await
    .map_err(|e| Error::WebSocket(e.to_string()))?;

    let (tx, rx) = tokio::sync::mpsc::channel(100);

    tokio::spawn(async move {
        let dump_raw = std::env::var_os("SINA_QUOTES_DUMP_WS").is_some();
        loop {
            match ws.read_frame().await {
                Ok(frame) => {
                    let opcode = frame.opcode;
                    if opcode == OpCode::Text || opcode == OpCode::Binary {
                        let bytes = frame.payload.to_vec();
                        let text = match String::from_utf8(bytes) {
                            Ok(text) => text,
                            Err(_) => continue,
                        };

                        if dump_raw {
                            println!("[WS RAW] {}", text);
                        }

                        for result in parse_quote_lines(&text, parse_fn) {
                            match result {
                                Ok(quote) => {
                                    let _ = tx.send(Ok(quote)).await;
                                }
                                Err(e) => {
                                    if dump_raw {
                                        println!("[WS RAW] parse error: {}", e);
                                    }
                                }
                            }
                        }
                    } else if opcode == OpCode::Close {
                        break;
                    }
                }
                Err(e) => {
                    let _ = tx.send(Err(Error::WebSocket(e.to_string()))).await;
                    break;
                }
            }
        }
    });

    Ok(tokio_stream::wrappers::ReceiverStream::new(rx))
}

pub async fn subscribe(
    symbols: &[&str],
) -> Result<impl futures_util::Stream<Item = std::result::Result<Quote, Error>> + use<>> {
    connect_and_stream(build_ws_url(symbols), parse_quote).await
}

pub async fn subscribe_international(
    symbols: &[&str],
) -> Result<impl futures_util::Stream<Item = std::result::Result<Quote, Error>> + use<>> {
    connect_and_stream(
        build_international_ws_url(symbols),
        parse_international_quote,
    )
    .await
}

fn parse_quote(text: &str) -> std::result::Result<Quote, Error> {
    let parts: Vec<&str> = text.split('=').collect();
    if parts.len() < 2 {
        return Err(Error::ChannelClosed);
    }

    let symbol = parts[0].trim().to_string();
    let data = parts[1].trim().trim_end_matches(';');

    let fields: Vec<&str> = data.split(',').collect();
    if fields.len() < 10 {
        return Err(Error::ChannelClosed);
    }

    Ok(Quote {
        symbol,
        price: fields[0].parse().unwrap_or(0.0),
        bid_price: 0.0,
        ask_price: 0.0,
        open: 0.0,
        high: 0.0,
        low: 0.0,
        prev_settle: 0.0,
        settle_price: 0.0,
        volume: fields[2].parse().unwrap_or(0.0),
        quote_time: String::new(),
        date: String::new(),
        name: String::new(),
        timestamp: chrono::Utc::now().timestamp(),
    })
}

fn parse_international_quote(text: &str) -> std::result::Result<Quote, Error> {
    let (symbol, rhs) = text
        .split_once('=')
        .ok_or_else(|| Error::Parse("no = pattern".to_string()))?;

    let symbol = symbol.trim().to_string();

    let mut data = rhs.trim().trim_end_matches(';').trim_end_matches(',');
    if let Some(stripped) = data.strip_prefix('"') {
        data = stripped.trim_end_matches('"');
    };

    if data.is_empty() {
        return Err(Error::ChannelClosed);
    }

    let fields: Vec<&str> = data.split(',').collect();
    if fields.len() < 10 {
        return Err(Error::Parse(format!(
            "insufficient fields: {}",
            fields.len()
        )));
    }

    let get = |i: usize| -> f64 { fields.get(i).and_then(|s| s.parse().ok()).unwrap_or(0.0) };

    let get_str = |i: usize| -> String { fields.get(i).map(|s| s.to_string()).unwrap_or_default() };

    let price = get(0);
    let bid_price = get(2);
    let ask_price = get(3);
    let high = get(4);
    let low = get(5);
    let quote_time = get_str(6);
    let prev_settle = get(7);
    let open = get(8);
    let volume = get(14);
    let date = get_str(12);
    let name = get_str(13);

    Ok(Quote {
        symbol,
        price,
        bid_price,
        ask_price,
        open,
        high,
        low,
        prev_settle,
        settle_price: 0.0,
        volume,
        quote_time,
        date,
        name,
        timestamp: chrono::Utc::now().timestamp(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_code_simple() {
        assert_eq!(normalize_code("sh510050"), "sh510050");
        assert_eq!(normalize_code("sh000001"), "sh000001");
    }

    #[test]
    fn test_normalize_code_with_dot() {
        assert_eq!(normalize_code("510050.SH"), "sh510050");
        assert_eq!(normalize_code("000001.SZ"), "sz000001");
    }

    #[test]
    fn test_normalize_code_empty() {
        assert_eq!(normalize_code(""), "");
        assert_eq!(normalize_code("   "), "");
    }

    #[test]
    fn test_build_ws_url_single() {
        let url = build_ws_url(&["sh510050"]);
        assert_eq!(url, "ws://w.sinajs.cn/wskt?list=sh510050");
    }

    #[test]
    fn test_build_ws_url_multiple() {
        let url = build_ws_url(&["sh510050", "sh510300"]);
        assert_eq!(url, "ws://w.sinajs.cn/wskt?list=sh510050,sh510300");
    }

    #[test]
    fn test_parse_quote_valid() {
        let text = "sh510050=2.930,0.01,123456789,100,2.920,2.940,2.910,2.930,1000000,10000000;";
        let result = parse_quote(text);
        assert!(result.is_ok());
        let quote = result.unwrap();
        assert_eq!(quote.symbol, "sh510050");
        assert_eq!(quote.price, 2.930);
        assert_eq!(quote.volume, 123456789.0);
    }

    #[test]
    fn test_parse_quote_invalid_no_equals() {
        let text = "sh510050";
        let result = parse_quote(text);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_quote_invalid_no_semicolon_data() {
        let text = "sh510050=";
        let result = parse_quote(text);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_quote_insufficient_fields() {
        let text = "sh510050=1.0,2.0;";
        let result = parse_quote(text);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_international_quote_frame_yields_all_lines() {
        let text = concat!(
            "hf_OIL=91.724,,92.420,92.490,98.980,86.090,05:59:57,99.390,98.450,0,10,12,2026-04-18,布伦特原油,582877\n",
            "hf_GC=4844.320,,4848.800,4849.700,4917.700,4785.900,04:59:59,4808.300,4811.800,0,2,1,2026-04-18,纽约黄金,0\n"
        );

        let quotes: Vec<Quote> = parse_quote_lines(text, parse_international_quote)
            .into_iter()
            .collect::<std::result::Result<_, _>>()
            .unwrap();

        assert_eq!(quotes.len(), 2);
        assert_eq!(quotes[0].symbol, "hf_OIL");
        assert_eq!(quotes[0].prev_settle, 99.390);
        assert_eq!(quotes[1].symbol, "hf_GC");
        assert_eq!(quotes[1].prev_settle, 4808.300);
    }
}
