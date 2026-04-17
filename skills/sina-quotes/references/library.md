# `sina-quotes` Rust API 参考

## 作用范围

这个库的历史 K 线和实时行情主要用于新浪财经外盘行情。

另外，交易时间段接口 `fetch_market_hours(symbol)` 会根据传入的 `symbol` 自动识别市场，并在客户端内按 TTL 缓存结果。

常见例子：

- `hf_OIL`：布伦特原油
- `hf_CL`：纽约原油
- `hf_GC`：纽约黄金
- `hf_SI`：纽约白银
- `hf_NG`：美国天然气

如果不确定可用符号，优先使用库内置的 `sina_quotes::symbols::ALL_SYMBOLS`。

## 依赖

```toml
[dependencies]
sina-quotes = { git = "https://github.com/zynthium/sina-quotes" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
```

## 任务选择

- 拉一段历史 K 线：`get_kline_serial`
- 订阅实时快照 Quote：`subscribe_quote` 或 `subscribe_quotes` + `start_websocket`
- 订阅实时 K 线：`subscribe_realtime_kline` + `start_websocket`
- 查询交易时间段：`fetch_market_hours`
- 想先手动试一下：看 `references/cli.md`

## 历史 K 线

```rust
use sina_quotes::{Duration, SinaQuotes};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = SinaQuotes::new().await?;

    let series = client
        .get_kline_serial("hf_OIL", Duration::minutes(5), 100)
        .await?;

    println!("symbol={}", series.symbol());
    println!("duration={}", series.duration());
    println!("len={}/{}", series.len(), series.capacity());

    for bar in series.read().iter().take(5) {
        println!("{}", bar);
    }

    client.close().await;
    Ok(())
}
```

使用建议：

- 默认优先使用 `Duration::minutes(1)`、`Duration::minutes(5)`、`Duration::minutes(15)`、`Duration::hours(1)`、`Duration::days(1)`
- `count` 表示希望保留的序列长度
- `KlineSeries` 是句柄，不是一次性返回的裸 `Vec<KlineBar>`

## 实时 Quote

```rust
use sina_quotes::SinaQuotes;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = SinaQuotes::new().await?;
    let symbol = "hf_GC";

    let mut stream = client.subscribe_quote(symbol).await?;

    client
        .start_websocket(vec![symbol.to_string()])
        .await?;

    for i in 0..10 {
        stream.changed().await?;
        let quote = stream.get();
        println!("[{i:02}] {} price={:.3}", quote.symbol, quote.price);
    }

    client.close().await;
    Ok(())
}
```

关键点：

- `subscribe_quote()` 本身不会建立到新浪的连接
- 真正的实时连接由 `start_websocket(...)` 启动
- 同一个客户端不要重复调用 `start_websocket(...)`

## 多品种实时 Quote

```rust
use sina_quotes::SinaQuotes;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = SinaQuotes::new().await?;

    let symbols = vec!["hf_OIL", "hf_GC"];
    let mut streams = client.subscribe_quotes(&symbols).await?;

    client
        .start_websocket(symbols.iter().map(|s| s.to_string()).collect())
        .await?;

    for _ in 0..10 {
        for stream in &mut streams {
            stream.changed().await?;
            let quote = stream.get();
            println!("{} price={:.3}", quote.symbol, quote.price);
        }
    }

    client.close().await;
    Ok(())
}
```

如果你一开始就知道要订阅多个品种，优先把所有符号一次性交给 `start_websocket(...)`。

## 实时 K 线

```rust
use sina_quotes::{Duration, SinaQuotes};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = SinaQuotes::new().await?;
    let symbol = "hf_OIL";

    client
        .start_websocket(vec![symbol.to_string()])
        .await?;

    let mut sub = client
        .subscribe_realtime_kline(symbol, Duration::minutes(1), 300)
        .await?;

    while let Some(ev) = sub.next().await {
        if ev.is_completed {
            println!("completed id={} close={:.3}", ev.bar.id, ev.bar.close);
        } else {
            println!("updating  id={} close={:.3}", ev.bar.id, ev.bar.close);
        }
    }

    client.close().await;
    Ok(())
}
```

关键点：

- `subscribe_realtime_kline()` 会维护 `RealtimeKline` 订阅句柄
- 它依赖 Quote 流来聚合 K 线，所以仍然需要 `start_websocket(...)`
- `ev.is_completed == false` 表示当前正在形成的 bar
- `ev.is_completed == true` 表示一根 bar 已经完成

## 交易时间段

```rust
use sina_quotes::SinaQuotes;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = SinaQuotes::new().await?;

    let market_hours = client.fetch_market_hours("SC0").await?;

    println!("category={}", market_hours.category);
    for session in &market_hours.sessions {
        println!("{} - {}", session.start, session.end);
    }

    client.close().await;
    Ok(())
}
```

关键点：

- 入口是 `client.fetch_market_hours(symbol).await?`
- 接口会根据 `symbol` 自动识别市场
- 同一个客户端内会按 TTL 复用结果，避免每次都请求远程接口
- 返回结果里的 `category` 会保留最终识别出的 `nf` / `hf`

## 可选配置

如果需要缓存或超时配置，使用 builder：

```rust
use sina_quotes::SinaQuotes;
use std::time::Duration as StdDuration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = SinaQuotes::builder()
        .http_timeout(StdDuration::from_secs(10))
        .default_data_length(300)
        .cache_dir("./cache".into())
        .market_hours_cache_ttl(StdDuration::from_secs(6 * 60 * 60))
        .build()
        .await?;

    println!("client ready");
    client.close().await;
    Ok(())
}
```

常用配置：

- `http_timeout(...)`
- `default_data_length(...)`
- `cache_dir(...)`
- `cache_capacity(...)`
- `market_hours_cache_ttl(...)`

## 常见坑

### 1. 把它当成通用股票库

不要这么写：“用这个库拉 AAPL 或上证指数。”

优先写清楚：这是新浪财经外盘行情 Rust 库，主要面向 `hf_` 外盘符号。

### 2. 漏掉 `start_websocket(...)`

这是最容易犯的错。只调用：

```rust
let mut stream = client.subscribe_quote("hf_OIL").await?;
```

还不会持续收到实时更新。必须再调用：

```rust
client.start_websocket(vec!["hf_OIL".to_string()]).await?;
```

### 3. 重复启动 WebSocket

同一个 `SinaQuotes` 客户端只保留一个 WebSocket 连接。要订阅多个外盘品种，启动前先收集所有符号。

### 4. 误读 Quote 字段

`Quote.open`、`Quote.high`、`Quote.low`、`Quote.volume` 是当日累计字段，不等于当前分钟 K 线的 OHLCV。

### 5. 忘记关闭客户端

短程序通常直接退出也能结束进程，但在长生命周期程序、测试或示例里，优先显式调用：

```rust
client.close().await;
```
