use sina_quotes::SinaQuotes;
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

    for i in 0..20 {
        quote_stream.changed().await?;
        let q = quote_stream.get();
        println!("[Q{:02}] {}", i, q);
    }

    client.close().await;
    Ok(())
}
