use clap::Parser;
use sina_quotes::{SinaQuotes, Duration};

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
    Subscribe {
        symbols: Vec<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    match Cmd::parse() {
        Cmd::Klines { symbol, period_minutes, count } => {
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
                println!("#{:03} O={:.3} H={:.3} L={:.3} C={:.3} V={:.0}",
                    bar.id, bar.open, bar.high, bar.low, bar.close, bar.volume);
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
            
            println!("Subscribed to {} symbols. Press Ctrl+C to exit.", symbols.len());
            
            // Keep the program running
            tokio::time::sleep(std::time::Duration::MAX).await;
        }
    }

    Ok(())
}
