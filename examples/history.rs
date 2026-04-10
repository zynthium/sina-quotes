use sina_quotes::{Duration, SinaQuotes};
use std::time::Duration as StdDuration;

fn parse_duration_arg(arg: Option<String>) -> Result<Duration, String> {
    match arg.as_deref().unwrap_or("1d") {
        "5m" => Ok(Duration::minutes(5)),
        "1d" => Ok(Duration::days(1)),
        "2d" => Ok(Duration::days(2)),
        "7d" => Ok(Duration::days(7)),
        other => Err(format!(
            "unsupported duration '{other}', use one of: 5m | 1d | 2d | 7d"
        )),
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let mut args = std::env::args().skip(1);
    let symbol = args.next().unwrap_or_else(|| "hf_OIL".to_string());
    let duration = parse_duration_arg(args.next())?;
    let count = args
        .next()
        .map(|v| v.parse())
        .transpose()?
        .unwrap_or(20usize);

    println!("usage: cargo run --example history [symbol] [5m|1d|2d|7d] [count]");
    println!("example: cargo run --example history hf_OIL 2d 10");

    let client = SinaQuotes::builder()
        .http_timeout(StdDuration::from_secs(10))
        .default_data_length(100)
        .build()
        .await?;

    let series = client.get_kline_serial(&symbol, duration, count).await?;

    println!("symbol={}", series.symbol());
    println!("duration={}", series.duration());
    println!("len={}/{}", series.len(), series.capacity());

    let bars = series.read();
    for (i, bar) in bars.iter().take(5).enumerate() {
        println!("[{}] {}", i, bar);
    }
    println!("... total={}", bars.len());

    client.close().await;
    Ok(())
}
