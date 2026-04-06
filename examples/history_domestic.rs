use sina_quotes::fetch_history;

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    println!("Fetching historical data for domestic futures (sh510050)...");
    
    let symbol = "sh510050"; // 510050.SH ETF
    
    match fetch_history(symbol, 5, 3).await {
        Ok(bars) => {
            println!("\nHistorical data:");
            for bar in bars {
                println!("- {}: close={} (open={}, high={}, low={}, volume={})", 
                    bar.time, bar.close, bar.open, bar.high, bar.low, bar.volume);
            }
        }
        Err(e) => {
            eprintln!("Failed to fetch history: {}", e);
        }
    }
}