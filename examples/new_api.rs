//! SinaQuotes SDK 示例
//!
//! 展示如何使用 SinaQuotes 客户端获取 K 线数据和实时行情。

use sina_quotes::{Duration, SinaQuotes};
use std::time::Duration as StdDuration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    println!("=== SinaQuotes SDK 示例 ===\n");

    // 1. 创建客户端
    println!("1. 创建客户端...");
    let client = SinaQuotes::builder()
        .http_timeout(StdDuration::from_secs(10))
        .default_data_length(100)
        .build()
        .await?;
    println!("   客户端创建成功\n");

    let symbol = "hf_OIL";
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "default".to_string());

    match mode.as_str() {
        "history" => run_history(&client, symbol).await?,
        "quote" => run_quote(&client, symbol).await?,
        "kline1m" => run_kline_1m(&client, symbol).await?,
        "combo" => run_combo(&client, symbol).await?,
        "cache" => run_cache().await?,
        _ => run_default(&client, symbol).await?,
    }

    println!("\n=== 完成 ===");
    client.close().await;
    Ok(())
}
async fn run_default(client: &SinaQuotes, symbol: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("Usage:");
    println!("  cargo run --example new_api history");
    println!("  cargo run --example new_api quote");
    println!("  cargo run --example new_api kline1m");
    println!("  cargo run --example new_api combo");
    println!("  cargo run --example new_api cache");
    println!();

    println!("默认模式：combo\n");
    run_combo(client, symbol).await
}

async fn run_history(client: &SinaQuotes, symbol: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("2. 获取 K 线序列...");
    let duration = Duration::minutes(5);

    let series = client
        .get_kline_serial(symbol, duration, 20)
        .await
        .map_err(|e| format!("获取 K 线失败: {}", e))?;

    let bars = series.read_all();
    println!("   符号: {}", series.symbol());
    println!("   周期: {}", series.duration());
    println!("   数量: {} / {}", bars.len(), series.capacity());

    println!("\n3. 读取 K 线数据...");
    for (i, bar) in bars.iter().take(5).enumerate() {
        println!("   [{}] {}", i, bar);
    }
    println!("   ... 共 {} 根\n", bars.len());
    Ok(())
}

async fn run_quote(client: &SinaQuotes, symbol: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("订阅实时行情（Quote）...");
    let mut quote_stream = client
        .subscribe_quote(symbol)
        .await
        .map_err(|e| format!("订阅失败: {}", e))?;

    client
        .start_websocket(vec![symbol.to_string()])
        .await
        .map_err(|e| format!("启动 WebSocket 失败: {}", e))?;

    println!("等待 20 次行情更新...\n");
    for i in 0..20 {
        match tokio::time::timeout(StdDuration::from_secs(10), quote_stream.changed()).await {
            Ok(Ok(())) => {}
            Ok(Err(err)) => return Err(err.into()),
            Err(_) => {
                println!("   10 秒内没有新行情，退出。");
                break;
            }
        }
        let q = quote_stream.get();
        println!("   [Q{:02}] {}", i, q);
    }
    Ok(())
}

async fn run_kline_1m(client: &SinaQuotes, symbol: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("订阅实时 1m K 线...");
    client
        .start_websocket(vec![symbol.to_string()])
        .await
        .map_err(|e| format!("启动 WebSocket 失败: {}", e))?;

    let mut sub = client
        .subscribe_realtime_kline(symbol, Duration::minutes(1), 300)
        .await
        .map_err(|e| format!("订阅实时 K 线失败: {}", e))?;

    for i in 0..100 {
        let ev = match tokio::time::timeout(StdDuration::from_secs(10), sub.next()).await {
            Ok(Some(ev)) => ev,
            Ok(None) => break,
            Err(_) => break,
        };
        if ev.is_completed {
            println!("   [K{:02}] 完结 {}", i, ev.bar);
        } else {
            println!("   [K{:02}] 更新 {}", i, ev.bar);
        }
    }
    Ok(())
}

async fn run_combo(client: &SinaQuotes, symbol: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("同时消费 Quote + RealtimeKline...\n");

    let mut quote_stream = client
        .subscribe_quote(symbol)
        .await
        .map_err(|e| format!("订阅失败: {}", e))?;

    client
        .start_websocket(vec![symbol.to_string()])
        .await
        .map_err(|e| format!("启动 WebSocket 失败: {}", e))?;

    let mut kline_sub = client
        .subscribe_realtime_kline(symbol, Duration::minutes(1), 300)
        .await
        .map_err(|e| format!("订阅实时 K 线失败: {}", e))?;

    let mut quote_update_count = 0;
    let max_quote_updates = 50;
    let mut kline_update_count = 0;
    let max_kline_updates = 50;
    let idle_timeout = StdDuration::from_secs(10);

    loop {
        let activity = tokio::time::timeout(idle_timeout, async {
            tokio::select! {
                res = quote_stream.changed() => {
                    if res.is_err() {
                        return false;
                    }
                    let q = quote_stream.get();
                    println!("   [Q{:02}] {}", quote_update_count, q);
                    quote_update_count += 1;
                    true
                }
                ev = kline_sub.next() => {
                    let Some(ev) = ev else { return false; };
                    if ev.is_completed {
                        println!("   [K{:02}] 完结 {}", kline_update_count, ev.bar);
                    } else {
                        println!("   [K{:02}] 更新 {}", kline_update_count, ev.bar);
                    }
                    kline_update_count += 1;
                    true
                }
            }
        })
        .await;

        match activity {
            Ok(true) => {}
            Ok(false) => break,
            Err(_) => {
                println!("   10 秒内没有新更新，退出。");
                break;
            }
        }

        if quote_update_count >= max_quote_updates && kline_update_count >= max_kline_updates {
            break;
        }
    }
    Ok(())
}

async fn run_cache() -> Result<(), Box<dyn std::error::Error>> {
    println!("启用本地缓存并拉取历史...");
    let client = SinaQuotes::builder()
        .cache_dir("./cache".into())
        .default_data_length(300)
        .build()
        .await?;

    let symbol = "hf_OIL";
    let duration = Duration::minutes(1);
    let series = client.get_kline_serial(symbol, duration, 300).await?;

    println!(
        "symbol={} duration={} len={} last_id={}",
        series.symbol(),
        series.duration(),
        series.len(),
        series.last_id()
    );

    if let Some(stats) = client.cache_stats() {
        println!(
            "cache entries={} total_bars={}",
            stats.total_entries, stats.total_bars
        );
    }

    client.close().await;
    Ok(())
}
