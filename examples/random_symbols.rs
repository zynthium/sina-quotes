use sina_quotes::ws;
use futures_util::StreamExt;
use rand::seq::SliceRandom;
use rand::thread_rng;

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    println!("Getting random domestic futures symbols for testing...");
    
    // This would normally fetch from a predefined list or API
    // For demo, we'll use some common domestic futures
    let symbols = [
        "sh510050",  // 510050.SH ETF
        "sh510300",  // 510300.SH ETF  
        "sz000001",  // 平安银行
        "sh600000",  // 浦发银行
    ];
    
    // Select 2 random symbols
    let mut rng = thread_rng();
    let selected: Vec<&str> = symbols.choose_multiple(&mut rng, 2).cloned().collect();
    
    println!("Selected symbols: {:?}", selected);
    
    match ws::subscribe(&selected).await {
        Ok(mut stream) => {
            println!("Subscribed! Waiting for data...");
            
            // Try to get a few updates
            let mut count = 0;
            while let Some(result) = stream.next().await {
                if count >= 3 { break; }
                
                match result {
                    Ok(quote) => {
                        println!("[{}] {}: {} (volume: {})", 
                            chrono::Utc::now().format("%H:%M:%S"),
                            quote.symbol, quote.price, quote.volume);
                        count += 1;
                    }
                    Err(e) => {
                        eprintln!("Error: {}", e);
                        break;
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to subscribe: {}", e);
        }
    }
}