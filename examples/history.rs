use sina_quotes::{Duration, SinaQuotes};
use std::time::Duration as StdDuration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let symbol = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "hf_OIL".to_string());

    let client = SinaQuotes::builder()
        .http_timeout(StdDuration::from_secs(10))
        .default_data_length(100)
        .build()
        .await?;

    let duration = Duration::minutes(5);
    let series = client.get_kline_serial(&symbol, duration, 20).await?;

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

