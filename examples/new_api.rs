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

    // 2. 获取 K 线序列
    println!("2. 获取 K 线序列...");
    let symbol = "hf_OIL";
    let duration = Duration::minutes(5);

    let series = client
        .get_kline_serial(symbol, duration, 20)
        .await
        .map_err(|e| format!("获取 K 线失败: {}", e))?;

    println!("   符号: {}", series.symbol());
    println!("   周期: {}", series.duration());
    println!("   数量: {} / {}", series.len(), series.capacity());

    // 3. 读取 K 线数据
    println!("\n3. 读取 K 线数据...");
    let bars = series.read();
    for (i, bar) in bars.iter().take(5).enumerate() {
        println!(
            "   [{}] ID:{} 开:{:.3} 高:{:.3} 低:{:.3} 收:{:.3}",
            i, bar.id, bar.open, bar.high, bar.low, bar.close
        );
    }
    println!("   ... 共 {} 根\n", bars.len());

    // 4. 订阅实时行情
    println!("4. 订阅实时行情...");
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

    println!("   订阅成功，等待行情更新...\n");

    // 5. 事件循环
    println!("5. 事件循环 (按 Ctrl+C 退出):\n");

    let mut update_count = 0;
    let max_updates = 50;
    let mut quote_update_count = 0;
    let max_quote_updates = 50;
    let mut kline_update_count = 0;
    let max_kline_updates = 50;

    loop {
        tokio::select! {
            _ = tokio::time::sleep(StdDuration::from_secs(2)) => {
                let _bars = series.read();
                if let Some(current) = series.current() {
                    println!("   [{:02}] 当前 K线 #{} 收盘: {:.3}",
                        update_count, current.id, current.close);
                }
                if let Some(last) = series.last() {
                    println!("   [{:02}] 最新完成 K线 #{} 收盘: {:.3}",
                        update_count, last.id, last.close);
                }

                update_count += 1;
                if update_count >= max_updates
                    && quote_update_count >= max_quote_updates
                    && kline_update_count >= max_kline_updates
                {
                    break;
                }
            }
            res = quote_stream.changed() => {
                if res.is_err() {
                    break;
                }
                let quote = quote_stream.get();
                println!("   [Q{:02}] {:#?}", quote_update_count, quote);
                quote_update_count += 1;
                if update_count >= max_updates
                    && quote_update_count >= max_quote_updates
                    && kline_update_count >= max_kline_updates
                {
                    break;
                }
            }
            ev = kline_sub.next() => {
                let Some(ev) = ev else {
                    break;
                };
                if ev.is_completed {
                    println!("   [K{:02}] 完结 bar id={} close={:.3}", kline_update_count, ev.bar.id, ev.bar.close);
                } else {
                    println!("   [K{:02}] 更新 bar id={} close={:.3}", kline_update_count, ev.bar.id, ev.bar.close);
                }
                kline_update_count += 1;
                if update_count >= max_updates
                    && quote_update_count >= max_quote_updates
                    && kline_update_count >= max_kline_updates
                {
                    break;
                }
            }
        }
    }

    println!("\n=== 完成 ===");
    client.close().await;
    Ok(())
}
