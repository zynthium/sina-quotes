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

### 完整示例

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
    
    // 订阅实时行情
    let _stream = client.subscribe_quote(symbol).await?;
    println!("Subscribed to {}", symbol);
    
    // 事件循环
    for i in 0..5 {
        tokio::time::sleep(StdDuration::from_secs(2)).await;
        
        if let Some(current) = series.current() {
            println!("[{}] Current: {:.3}", i, current.close);
        }
        if let Some(last) = series.last() {
            println!("[{}] Last: {:.3}", i, last.close);
        }
    }
    
    Ok(())
}
```

## License

MIT
