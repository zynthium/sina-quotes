use sina_quotes::fetch_history;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    
    println!("Fetching historical data for international futures (hf_OIL)...");
    
    let symbol = "hf_OIL";
    
    match fetch_history(symbol, 5, 3).await {
        Ok(bars) => {
            println!("\nHistorical data:");
            for bar in bars.iter().take(5) {
                println!("- {}: open={:.3}, high={:.3}, low={:.3}, close={:.3}, vol={:.0}, oi={:.0}", 
                    bar.time, bar.open, bar.high, bar.low, bar.close, bar.volume, bar.open_interest);
            }
            println!("Total: {} bars", bars.len());
        }
        Err(e) => {
            eprintln!("Failed to fetch history: {}", e);
        }
    }
}