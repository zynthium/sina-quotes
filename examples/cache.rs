use sina_quotes::{Duration, SinaQuotes};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let client = SinaQuotes::builder()
        .cache_dir("./cache".into())
        .default_data_length(300)
        .build()
        .await?;

    let symbol = "hf_OIL";
    let duration = Duration::minutes(1);
    let series = client.get_kline_serial(symbol, duration, 300).await?;

    println!("symbol={}", series.symbol());
    println!("duration={}", series.duration());
    println!("len={}", series.len());
    println!("last_id={}", series.last_id());

    if let Some(stats) = client.cache_stats() {
        println!(
            "cache entries={} total_bars={}",
            stats.total_entries, stats.total_bars
        );
    }

    client.close().await;
    Ok(())
}
