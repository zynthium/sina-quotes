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

    let mut quote_stream = client.subscribe_quote(&symbol).await?;
    client.start_websocket(vec![symbol.clone()]).await?;

    let mut kline_sub = client
        .subscribe_realtime_kline(&symbol, Duration::minutes(1), 300)
        .await?;

    let mut q_count = 0usize;
    let mut k_count = 0usize;
    loop {
        tokio::select! {
            r = quote_stream.changed() => {
                if r.is_err() { break; }
                let q = quote_stream.get();
                println!("[Q{:02}] {}", q_count, q);
                q_count += 1;
            }
            ev = kline_sub.next() => {
                let Some(ev) = ev else { break; };
                if ev.is_completed {
                    println!("[K{:02}] 完结 {}", k_count, ev.bar);
                } else {
                    println!("[K{:02}] 更新 {}", k_count, ev.bar);
                }
                k_count += 1;
            }
        }
        if q_count >= 50 && k_count >= 50 {
            break;
        }
    }

    client.close().await;
    Ok(())
}
