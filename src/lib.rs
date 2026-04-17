//! SinaQuotes SDK - 新浪财经数据 Rust SDK
//!
//! 支持历史 K 线数据获取和实时行情 WebSocket 订阅。
//!
//! # 核心类型
//!
//! - [`SinaQuotes`] - SDK 客户端入口
//! - [`KlineSeries`] - K 线序列（类似 TqSdk）
//! - [`QuoteStream`] - 实时行情流
//! - [`KlineBar`] - K 线柱数据
//! - [`Quote`] - 行情数据
//! - [`Duration`] - 时间周期
//!
//! # 快速开始
//!
//! ```rust,no_run
//! use sina_quotes::{SinaQuotes, Duration};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), sina_quotes::SdkError> {
//!     let client = SinaQuotes::new().await?;
//!
//!     // 获取 K 线序列
//!     let series = client
//!         .get_kline_serial("hf_CL", Duration::minutes(5), 100)
//!         .await?;
//!
//!     // 读取数据
//!     for bar in series.read() {
//!         println!("#{:03} O={:.2} H={:.2} L={:.2} C={:.2}",
//!             bar.id, bar.open, bar.high, bar.low, bar.close);
//!     }
//!
//!     Ok(())
//! }
//! ```

// ===================== 模块 =====================

pub mod client; // 客户端入口
pub mod data; // 数据层：类型、缓冲区、K线序列
pub mod error; // 错误类型
pub mod net; // 网络层：WebSocket、历史数据
pub mod realtime_kline;
pub mod storage; // 存储层：缓存、范围集合
pub mod stream; // 实时行情流
pub mod symbols; // 外盘期货品种符号

// ===================== 重新导出 =====================

// 核心类型
pub use client::{ClientBuilder, ClientConfig, SinaQuotes};
pub use data::buffer::{KlineRingBuffer, RingBuffer};
pub use data::series::KlineSeries;
pub use data::types::{
    Duration, FuturesCategory, FuturesMarketHours, KlineBar, KlineData, Quote, TradingSession,
};
pub use error::{Result, SdkError};
pub use realtime_kline::{KlineEvent, RealtimeKline};
pub use storage::cache::{CacheEntryMeta, CacheKey, CacheStats, HistoryCache};
pub use storage::rangeset::{Range, RangeSet, rangeset_difference, rangeset_union};
pub use stream::QuoteStream;

// ===================== 版本信息 =====================

/// SDK 版本
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 检查 SDK 版本
pub fn version() -> &'static str {
    VERSION
}

// ===================== 单元测试 =====================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_duration() {
        let d = Duration::minutes(5);
        assert_eq!(d.as_secs(), 300);
        assert_eq!(d.as_minutes(), 5);
    }

    #[test]
    fn test_kline_bar() {
        let bar = KlineBar {
            id: 1,
            datetime: 1704067200000000000,
            open: 100.0,
            high: 105.0,
            low: 95.0,
            close: 102.0,
            volume: 1000.0,
            open_interest: 5000.0,
        };

        assert_eq!(bar.id, 1);
        assert_eq!(bar.close, 102.0);
    }
}
