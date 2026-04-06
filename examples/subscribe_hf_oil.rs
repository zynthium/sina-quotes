use futures_util::StreamExt;
use sina_quotes::{ws, poll};
use chrono::Utc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    
    println!("Subscribing to hf_OIL (international crude oil futures) with auto-reconnect...");
    println!("Note: WebSocket connections to sina finance may be intermittent");
    println!("This example will attempt to connect and show real-time data\n");
    
    let symbols = ["hf_OIL"];
    let mut reconnect_attempts = 0;
    const MAX_RECONNECT_ATTEMPTS: u32 = 5;
    
    loop {
        match ws::subscribe_international(&symbols).await {
            Ok(mut stream) => {
                println!("Connected! Waiting for real-time data (attempt {})...", reconnect_attempts + 1);
                reconnect_attempts = 0;
                
                while let Some(result) = stream.next().await {
                    match result {
                        Ok(quote) => {
                            let change: f64 = quote.price.parse::<f64>().unwrap_or(0.0) 
                                - quote.prev_settle.parse::<f64>().unwrap_or(0.0);
                            let change_percent = if quote.prev_settle.parse::<f64>().unwrap_or(0.0) > 0.0 {
                                change / quote.prev_settle.parse::<f64>().unwrap_or(1.0) * 100.0
                            } else { 0.0 };
                            
                            println!("\n[{}] {} ({})",
                                Utc::now().format("%H:%M:%S"),
                                quote.symbol,
                                quote.name);
                            println!("  最新价: {} 涨跌: {:.3} {:.2}%", quote.price, change, change_percent);
                            println!("  开盘: {} 最高: {} 最低: {}", quote.open, quote.high, quote.low);
                            println!("  结算价: {} 昨结: {} 持仓: {}", quote.settle_price, quote.prev_settle, quote.volume);
                            println!("  买价: {} 卖价: {} 时间: {} {}", quote.bid_price, quote.ask_price, quote.quote_time, quote.date);
                        }
                        Err(e) => {
                            eprintln!("\nError receiving data: {}", e);
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("\nFailed to connect: {}", e);
                reconnect_attempts += 1;
                
                if reconnect_attempts >= MAX_RECONNECT_ATTEMPTS {
                    eprintln!("Max reconnection attempts reached. Falling back to HTTP polling...");
                    
                    match poll::poll_international(&symbols, 5).await {
                        Ok(mut stream) => {
                            println!("\nSuccessfully fetched via HTTP:");
                            let mut count = 0;
                            while let Some(result) = stream.next().await {
                                if count >= 3 { break; }
                                match result {
                                    Ok(quote) => {
                                        println!("- {}: {} volume: {}", 
                                            quote.symbol, quote.price, quote.volume);
                                        count += 1;
                                    }
                                    Err(e) => {
                                        eprintln!("Error in HTTP stream: {}", e);
                                        break;
                                    }
                                }
                            }
                        }
                        Err(http_err) => {
                            eprintln!("HTTP fallback also failed: {}", http_err);
                        }
                    }
                    break;
                }
                
                if reconnect_attempts < MAX_RECONNECT_ATTEMPTS {
                    let delay_secs = 2u64.pow(reconnect_attempts);
                    println!("Reconnecting in {} seconds... (attempt {}/{})", 
                        delay_secs, reconnect_attempts + 1, MAX_RECONNECT_ATTEMPTS);
                    tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;
                }
            }
        }
    }
}
