# SinaQuotes SDK

新浪财经数据 Rust SDK，支持历史 K 线数据获取和实时行情 WebSocket 订阅。

## 特性

- **历史 K 线数据** - 获取分钟级 K 线数据，自动缓存
- **实时行情订阅** - WebSocket 实时推送，支持自动重连
- **K 线序列管理** - 类似 TqSdk 的 `get_kline_serial()` 接口
- **高性能** - 环形缓冲区、RangeSet 缓存优化

## 安装

```toml
[dependencies]
sina-quotes = { git = "https://github.com/zynthium/sina-quotes" }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
tracing-subscriber = "0.3"
```

## 快速开始

```rust
use sina_quotes::{SinaQuotes, Duration};

#[tokio::main]
async fn main() -> Result<(), sina_quotes::SdkError> {
    let client = SinaQuotes::new().await?;
    
    // 获取 K 线序列
    let series = client
        .get_kline_serial("hf_CL", Duration::minutes(5), 100)
        .await?;
    
    // 读取数据
    for bar in series.read() {
        println!("#{:03} O={:.2} H={:.2} L={:.2} C={:.2}",
            bar.id, bar.open, bar.high, bar.low, bar.close);
    }
    
    Ok(())
}
```

## 核心概念

### SinaQuotes 客户端

统一的客户端入口，管理所有数据源和缓存：

```rust
// 默认配置
let client = SinaQuotes::new().await?;

// 自定义配置
let client = SinaQuotes::builder()
    .http_timeout(Duration::from_secs(30))
    .default_data_length(200)
    .cache_dir("./cache".into())
    .build()
    .await?;
```

### K 线序列 (KlineSeries)

类似 TqSdk 的 K 线序列，支持实时更新：

```rust
let series = client
    .get_kline_serial("hf_OIL", Duration::minutes(1), 100)
    .await?;

// 读取所有数据
let bars = series.read();
for bar in bars.iter() {
    println!("{:?}", bar);
}

// 获取最新完成 K 线
if let Some(last) = series.last() {
    println!("最新收盘: {:.2}", last.close);
}

// 获取当前正在形成的 K 线
if let Some(current) = series.current() {
    println!("当前价: {:.2}", current.close);
}
```

### 实时行情 (QuoteStream)

订阅实时行情数据：

```rust
// 订阅单个
let stream = client.subscribe_quote("hf_CL").await?;

// 订阅多个
let streams = client.subscribe_quotes(&["hf_OIL", "hf_GC"]).await?;

// 获取当前行情
let quote = stream.borrow();
println!("{} 价格: {:.2}", quote.symbol, quote.price);
```

## API 参考

### 核心类型

| 类型 | 说明 |
|------|------|
| `SinaQuotes` | SDK 客户端入口 |
| `SinaQuotes::builder()` | 配置构建器 |
| `KlineSeries` | K 线序列 |
| `QuoteStream` | 实时行情流 |
| `KlineBar` | K 线柱数据 |
| `Quote` | 行情数据 |
| `Duration` | 时间周期 |

### Duration 周期

```rust
Duration::seconds(60)    // 1 秒 (不支持)
Duration::minutes(1)     // 1 分钟
Duration::minutes(5)     // 5 分钟
Duration::minutes(15)    // 15 分钟
Duration::hours(1)       // 1 小时
Duration::days(1)        // 1 天
```

### 错误处理

```rust
#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    #[error("HTTP request failed: {0}")]
    Http(reqwest::Error),
    
    #[error("parse error: {0}")]
    Parse(String),
    
    #[error("WebSocket error: {0}")]
    WebSocket(String),
    
    #[error("history data unavailable: {0}")]
    HistoryUnavailable(String),
    
    #[error("client closed")]
    Closed,
}
```

## CLI 使用

```bash
# 获取 K 线数据
cargo run -- klines hf_OIL 5

# 带参数
cargo run -- klines hf_GC 15 --count 200

# 订阅实时行情
cargo run -- subscribe hf_OIL hf_GC
```

## 数据类型

### KlineBar

```rust
pub struct KlineBar {
    pub id: i64,           // K 线 ID (用于排序)
    pub datetime: i64,     // 时间戳 (纳秒)
    pub open: f64,         // 开盘价
    pub high: f64,         // 最高价
    pub low: f64,          // 最低价
    pub close: f64,        // 收盘价
    pub volume: f64,      // 成交量
    pub open_interest: f64, // 持仓量
}
```

### Quote

```rust
pub struct Quote {
    pub symbol: String,        // 合约代码
    pub price: f64,           // 最新价
    pub bid_price: f64,        // 买价
    pub ask_price: f64,       // 卖价
    pub open: f64,            // 开盘价
    pub high: f64,            // 最高价
    pub low: f64,             // 最低价
    pub prev_settle: f64,     // 昨结算
    pub settle_price: f64,    // 结算价
    pub volume: f64,         // 成交量
    pub quote_time: String,   // 行情时间
    pub date: String,         // 日期
    pub name: String,         // 合约名称
    pub timestamp: i64,       // 时间戳
}
```

## 项目结构

```
src/
├── lib.rs              # 模块导出
├── main.rs             # CLI 入口
├── client.rs           # SinaQuotes 客户端
├── stream.rs           # QuoteStream 行情流
├── symbols.rs          # 外盘期货品种符号
├── error.rs            # 错误定义
├── data/               # 数据层
│   ├── types.rs        # KlineBar, Quote, Duration
│   ├── buffer.rs       # 环形缓冲区
│   └── series.rs       # KlineSeries K 线序列
├── net/                # 网络层
│   ├── ws.rs           # WebSocket 协议
│   ├── ws_service.rs   # WebSocket 连接管理
│   └── history.rs      # 历史数据获取
└── storage/            # 存储层
    ├── cache.rs        # 缓存管理
    └── rangeset.rs     # RangeSet
```

## 示例

### 1. 仅获取历史 K 线

```rust
use std::time::Duration as StdDuration;
use sina_quotes::{SinaQuotes, Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    
    let client = SinaQuotes::builder()
        .http_timeout(StdDuration::from_secs(10))
        .default_data_length(100)
        .build()
        .await?;

    // 获取 K 线
    let symbol = "hf_OIL";
    let series = client
        .get_kline_serial(symbol, Duration::minutes(5), 20)
        .await?;
    
    println!("Symbol: {}", series.symbol());
    println!("Duration: {}", series.duration());
    println!("Bars: {} / {}", series.len(), series.capacity());

    Ok(())
}
```

### 2. 订阅实时行情（QuoteStream，需要启动 WebSocket）

```rust
use sina_quotes::{Duration, SinaQuotes};
use std::time::Duration as StdDuration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = SinaQuotes::new().await?;

    let symbol = "hf_OIL";

    // 订阅行情流（快照）
    let mut stream = client.subscribe_quote(symbol).await?;

    // 启动 WebSocket（真正建立到新浪的连接）
    client
        .start_websocket(vec![symbol.to_string()])
        .await?;

    // 等待并打印 10 次更新
    for i in 0..10 {
        stream.changed().await?;
        let quote = stream.get();
        println!("[{:02}] {} price={:.2}", i, quote.symbol, quote.price);
    }

    Ok(())
}
```

### 3. 订阅实时 1 分钟 K 线（RealtimeKline）

```rust
use sina_quotes::{Duration, SinaQuotes};
use std::time::Duration as StdDuration;

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

    // 事件循环：未完结 / 已完结 bar 都会推送
    while let Some(ev) = sub.next().await {
        if ev.is_completed {
            println!("新完成 1m K 线: id={} close={:.3}", ev.bar.id, ev.bar.close);
        } else {
            println!("当前 1m K 线更新: id={} close={:.3}", ev.bar.id, ev.bar.close);
        }
    }

    Ok(())
}
```

### 4. 同时消费 Quote 与 RealtimeKline

```rust
use sina_quotes::{Duration, SinaQuotes};
use std::time::Duration as StdDuration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = SinaQuotes::new().await?;
    let symbol = "hf_OIL";

    let mut quote_stream = client.subscribe_quote(symbol).await?;
    client
        .start_websocket(vec![symbol.to_string()])
        .await?;

    let mut kline_sub = client
        .subscribe_realtime_kline(symbol, Duration::minutes(1), 300)
        .await?;

    loop {
        tokio::select! {
            res = quote_stream.changed() => {
                if res.is_err() {
                    break;
                }
                let q = quote_stream.get();
                println!("[Q] {} price={:.2}", q.symbol, q.price);
            }
            ev = kline_sub.next() => {
                let Some(ev) = ev else { break; };
                if ev.is_completed {
                    println!("[K] 完成 bar id={} close={:.3}", ev.bar.id, ev.bar.close);
                } else {
                    println!("[K] 更新 bar id={} close={:.3}", ev.bar.id, ev.bar.close);
                }
            }
        }
    }

    Ok(())
}
```

### 5. 启用本地缓存并复用历史数据

```rust
use sina_quotes::{Duration, SinaQuotes};
use std::time::Duration as StdDuration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 第一次运行：会从服务器拉取历史并写入缓存目录
    let client = SinaQuotes::builder()
        .cache_dir("./cache".into())
        .default_data_length(300)
        .build()
        .await?;

    let symbol = "hf_OIL";
    let duration = Duration::minutes(1);

    let series = client
        .get_kline_serial(symbol, duration, 300)
        .await?;

    println!(
        "第一次：symbol={} len={} last_id={}",
        series.symbol(),
        series.len(),
        series.last_id()
    );

    if let Some(stats) = client.cache_stats() {
        println!("缓存条目数={} 总 bars={}", stats.total_entries, stats.total_bars);
    }

    Ok(())
}
```

## License

MIT
