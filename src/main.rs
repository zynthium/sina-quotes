use clap::Parser;
use sina_quotes::{
    Duration, FuturesMarketHours, KlineBar, KlineSeries, Quote, QuoteStream, SinaQuotes,
};
use std::collections::HashMap;
use std::time::{Duration as StdDuration, Instant};

#[derive(Parser, Debug)]
#[command(name = "sina-quotes")]
enum Cmd {
    /// 获取 K 线数据
    Klines {
        symbol: String,
        #[arg(default_value = "5")]
        period_minutes: u32,
        #[arg(short, long, default_value = "100")]
        count: usize,
    },
    /// 订阅实时行情
    Subscribe { symbols: Vec<String> },
    /// 获取期货交易时间段
    MarketHours { symbol: String },
}

type QuoteFingerprint = (i64, u64, u64, u64, u64);

fn history_bars_for_display(series: &KlineSeries) -> Vec<KlineBar> {
    series.read_all()
}

fn quote_has_payload(quote: &Quote) -> bool {
    quote.price != 0.0
        || quote.bid_price != 0.0
        || quote.ask_price != 0.0
        || quote.open != 0.0
        || quote.high != 0.0
        || quote.low != 0.0
        || quote.volume != 0.0
        || quote.prev_settle != 0.0
        || quote.settle_price != 0.0
        || !quote.quote_time.is_empty()
        || !quote.date.is_empty()
        || !quote.name.is_empty()
}

fn quote_fingerprint(quote: &Quote) -> QuoteFingerprint {
    (
        quote.timestamp,
        quote.price.to_bits(),
        quote.bid_price.to_bits(),
        quote.ask_price.to_bits(),
        quote.volume.to_bits(),
    )
}

fn collect_fresh_quotes(
    streams: &[QuoteStream],
    seen: &mut HashMap<String, QuoteFingerprint>,
) -> Vec<Quote> {
    let mut fresh = Vec::new();

    for stream in streams {
        let quote = stream.get();
        if !quote_has_payload(&quote) {
            continue;
        }

        let fingerprint = quote_fingerprint(&quote);
        if seen.get(&quote.symbol) == Some(&fingerprint) {
            continue;
        }

        seen.insert(quote.symbol.clone(), fingerprint);
        fresh.push(quote);
    }

    fresh
}

async fn run_subscribe(
    client: &SinaQuotes,
    symbols: &[String],
    max_updates: usize,
    idle_timeout: StdDuration,
) -> anyhow::Result<()> {
    if symbols.is_empty() {
        anyhow::bail!("at least one symbol is required");
    }

    let symbol_refs: Vec<&str> = symbols.iter().map(String::as_str).collect();
    let streams = client.subscribe_quotes(&symbol_refs).await?;
    client.start_websocket(symbols.to_vec()).await?;

    println!(
        "Subscribed to {} symbols. Waiting for quote updates...",
        symbols.len()
    );

    let mut seen = HashMap::new();
    let mut printed = 0usize;
    let mut last_activity = Instant::now();

    loop {
        let fresh = collect_fresh_quotes(&streams, &mut seen);
        if !fresh.is_empty() {
            for quote in fresh {
                println!("{}", quote);
                printed += 1;
            }
            last_activity = Instant::now();
            if printed >= max_updates {
                break;
            }
        } else if last_activity.elapsed() >= idle_timeout {
            println!("No quote updates for {:?}, exiting.", idle_timeout);
            break;
        }

        tokio::time::sleep(StdDuration::from_millis(200)).await;
    }

    Ok(())
}

fn render_market_hours(market_hours: &FuturesMarketHours) -> String {
    let mut out = String::new();
    out.push_str(&format!("Category: {}\n", market_hours.category));
    out.push_str(&format!("Symbol: {}\n", market_hours.symbol));
    out.push_str(&format!("Timezone: {}\n", market_hours.timezone));

    if let Some(exchange) = &market_hours.exchange {
        out.push_str(&format!("Exchange: {}\n", exchange));
    }

    if let Some(interval) = &market_hours.interval {
        out.push_str(&format!("Interval: {}\n", interval));
    }

    out.push_str("Sessions:\n");
    for (index, session) in market_hours.sessions.iter().enumerate() {
        out.push_str(&format!(
            "  {}. {} - {}\n",
            index + 1,
            session.start,
            session.end
        ));
    }

    out
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    match Cmd::parse() {
        Cmd::Klines {
            symbol,
            period_minutes,
            count,
        } => {
            tracing::info!("fetching {symbol} {period_minutes}min history...");

            let client = SinaQuotes::new().await?;
            let duration = Duration::minutes(period_minutes as u64);
            let series = client.get_kline_serial(&symbol, duration, count).await?;
            let bars = history_bars_for_display(&series);

            println!("Symbol: {}", series.symbol());
            println!("Duration: {}", series.duration());
            println!("Bars: {} / {}", bars.len(), series.capacity());
            println!();

            for bar in bars.iter().take(10) {
                println!(
                    "#{:03} O={:.3} H={:.3} L={:.3} C={:.3} V={:.0}",
                    bar.id, bar.open, bar.high, bar.low, bar.close, bar.volume
                );
            }

            if bars.len() > 10 {
                println!("... and {} more", bars.len() - 10);
            }
        }
        Cmd::Subscribe { symbols } => {
            tracing::info!("subscribing to {:?}", symbols);
            let client = SinaQuotes::new().await?;
            run_subscribe(&client, &symbols, 20, StdDuration::from_secs(10)).await?;
            client.close().await;
        }
        Cmd::MarketHours { symbol } => {
            tracing::info!("fetching market hours for {symbol}");

            let client = SinaQuotes::new().await?;
            let market_hours = client.fetch_market_hours(&symbol).await?;

            print!("{}", render_market_hours(&market_hours));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sina_quotes::{
        FuturesCategory, FuturesMarketHours, KlineBar, KlineSeries, Quote, QuoteStream,
        TradingSession,
    };
    use tokio::sync::watch;

    #[test]
    fn test_parse_market_hours_command_with_bare_symbol() {
        let cmd = Cmd::try_parse_from(["sina-quotes", "market-hours", "SC0"]).unwrap();

        match cmd {
            Cmd::MarketHours { symbol } => {
                assert_eq!(symbol, "SC0");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn test_parse_market_hours_command_with_prefixed_symbol() {
        let cmd = Cmd::try_parse_from(["sina-quotes", "market-hours", "hf_FEF"]).unwrap();

        match cmd {
            Cmd::MarketHours { symbol } => {
                assert_eq!(symbol, "hf_FEF");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn test_render_market_hours_output() {
        let output = render_market_hours(&FuturesMarketHours {
            category: FuturesCategory::Hf,
            symbol: "FEF".to_string(),
            timezone: "summer".to_string(),
            exchange: Some("(SGX)".to_string()),
            interval: None,
            sessions: vec![
                TradingSession {
                    start: "07:25".to_string(),
                    end: "23:59".to_string(),
                },
                TradingSession {
                    start: "00:00".to_string(),
                    end: "05:15".to_string(),
                },
            ],
        });

        assert_eq!(
            output,
            "Category: hf\nSymbol: FEF\nTimezone: summer\nExchange: (SGX)\nSessions:\n  1. 07:25 - 23:59\n  2. 00:00 - 05:15\n"
        );
    }

    #[test]
    fn test_history_bars_for_display_includes_current_bar() {
        let series = KlineSeries::new("hf_TEST".to_string(), Duration::minutes(5), 16);
        series.push(KlineBar {
            id: 1,
            datetime: 0,
            open: 1.0,
            high: 1.0,
            low: 1.0,
            close: 1.0,
            volume: 1.0,
            open_interest: 0.0,
        });
        series.push(KlineBar {
            id: 2,
            datetime: 300 * 1_000_000_000,
            open: 2.0,
            high: 2.0,
            low: 2.0,
            close: 2.0,
            volume: 2.0,
            open_interest: 0.0,
        });

        assert_eq!(series.read().len(), 1);
        assert_eq!(history_bars_for_display(&series).len(), 2);
    }

    #[test]
    fn test_collect_fresh_quotes_ignores_placeholders_and_duplicates() {
        let (tx1, rx1) = watch::channel(Quote {
            symbol: "hf_OIL".to_string(),
            ..Default::default()
        });
        let (tx2, rx2) = watch::channel(Quote {
            symbol: "hf_GC".to_string(),
            ..Default::default()
        });
        let streams = vec![QuoteStream::new(rx1), QuoteStream::new(rx2)];
        let mut seen = std::collections::HashMap::new();

        assert!(collect_fresh_quotes(&streams, &mut seen).is_empty());

        tx1.send(Quote {
            symbol: "hf_OIL".to_string(),
            timestamp: 1,
            price: 91.2,
            volume: 10.0,
            ..Default::default()
        })
        .unwrap();
        tx2.send(Quote {
            symbol: "hf_GC".to_string(),
            timestamp: 2,
            price: 3300.0,
            volume: 12.0,
            ..Default::default()
        })
        .unwrap();

        let fresh = collect_fresh_quotes(&streams, &mut seen);
        assert_eq!(fresh.len(), 2);
        assert_eq!(fresh[0].symbol, "hf_OIL");
        assert_eq!(fresh[1].symbol, "hf_GC");
        assert!(collect_fresh_quotes(&streams, &mut seen).is_empty());
    }
}
