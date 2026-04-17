use clap::Parser;
use sina_quotes::{Duration, FuturesMarketHours, SinaQuotes};

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

            println!("Symbol: {}", series.symbol());
            println!("Duration: {}", series.duration());
            println!("Bars: {} / {}", series.len(), series.capacity());
            println!();

            let bars = series.read();
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

            for symbol in &symbols {
                client.subscribe_quote(symbol).await?;
            }

            println!(
                "Subscribed to {} symbols. Press Ctrl+C to exit.",
                symbols.len()
            );

            // Keep the program running
            tokio::time::sleep(std::time::Duration::MAX).await;
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
    use sina_quotes::{FuturesCategory, FuturesMarketHours, TradingSession};

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
}
