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

    client.start_websocket(vec![symbol.clone()]).await?;

    let mut sub = client
        .subscribe_realtime_kline(&symbol, Duration::minutes(1), 300)
        .await?;

    for i in 0..100 {
        let ev = match tokio::time::timeout(StdDuration::from_secs(10), sub.next()).await {
            Ok(Some(ev)) => ev,
            _ => break,
        };
        if ev.is_completed {
            println!("[K{:02}] 完结 {}", i, ev.bar);
        } else {
            println!("[K{:02}] 更新 {}", i, ev.bar);
        }
    }

    client.close().await;
    Ok(())
}
