//! 数据类型定义

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 时间周期
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Duration {
    /// 纳秒数
    pub ns: u64,
}

impl Duration {
    /// 秒数
    pub fn secs(n: u64) -> Self {
        Self {
            ns: n * 1_000_000_000,
        }
    }

    /// 分钟
    pub fn minutes(n: u64) -> Self {
        Self::secs(n * 60)
    }

    /// 小时
    pub fn hours(n: u64) -> Self {
        Self::minutes(n * 60)
    }

    /// 天
    pub fn days(n: u64) -> Self {
        Self::hours(n * 24)
    }

    /// 转换为秒数
    pub fn as_secs(&self) -> u64 {
        self.ns / 1_000_000_000
    }

    /// 转换为分钟数
    pub fn as_minutes(&self) -> u64 {
        self.as_secs() / 60
    }

    /// 检查是否为分钟周期
    pub fn is_minute_period(&self) -> bool {
        let secs = self.as_secs();
        secs > 0 && secs.is_multiple_of(60) && secs <= 1440
    }
}

impl Default for Duration {
    fn default() -> Self {
        Self::minutes(5)
    }
}

impl std::fmt::Display for Duration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.ns.is_multiple_of(1_000_000_000) {
            write!(f, "{}s", self.ns / 1_000_000_000)
        } else if self.ns.is_multiple_of(1_000_000) {
            write!(f, "{}ms", self.ns / 1_000_000)
        } else {
            write!(f, "{}ns", self.ns)
        }
    }
}

/// K线柱
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KlineBar {
    /// K线ID（可用于排序和去重）
    pub id: i64,
    /// 时间戳（纳秒）
    pub datetime: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    /// 持仓量（open interest）
    pub open_interest: f64,
}

impl KlineBar {
    /// 从新浪API的历史数据创建
    pub fn from_sina_fields(
        time: &str,
        open: &str,
        high: &str,
        low: &str,
        close: &str,
        volume: &str,
        open_interest: &str,
    ) -> Option<Self> {
        Some(Self {
            id: 0, // 需要从外部传入
            datetime: Self::parse_datetime(time)?,
            open: open.parse().ok()?,
            high: high.parse().ok()?,
            low: low.parse().ok()?,
            close: close.parse().ok()?,
            volume: volume.parse().ok()?,
            open_interest: open_interest.parse().ok()?,
        })
    }

    /// 解析新浪格式的时间字符串
    fn parse_datetime(s: &str) -> Option<i64> {
        // 尝试多种时间格式
        let formats = ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%d"];

        for fmt in formats {
            if let Ok(dt) = DateTime::parse_from_str(s, fmt) {
                return dt.timestamp_nanos_opt();
            }
        }
        None
    }

    /// 获取日期时间
    pub fn datetime_utc(&self) -> DateTime<Utc> {
        DateTime::from_timestamp(self.datetime / 1_000_000_000, 0).unwrap_or_default()
    }
}

/// K线数据结构（包含多个Bar）
#[derive(Debug, Clone)]
pub struct KlineData {
    pub symbol: String,
    pub duration: Duration,
    pub bars: Vec<KlineBar>,
}

impl KlineData {
    pub fn new(symbol: String, duration: Duration) -> Self {
        Self {
            symbol,
            duration,
            bars: Vec::new(),
        }
    }

    pub fn last(&self) -> Option<&KlineBar> {
        self.bars.last()
    }

    pub fn len(&self) -> usize {
        self.bars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bars.is_empty()
    }
}

/// 实时行情数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quote {
    /// 证券代码
    pub symbol: String,
    /// 最新价
    pub price: f64,
    /// 买一价
    pub bid_price: f64,
    /// 卖一价
    pub ask_price: f64,
    /// 今日开盘价（注意：非当前 K 线的开盘价，而是当日累计）
    pub open: f64,
    /// 今日最高价（注意：非当前 K 线的最高价，而是当日累计）
    pub high: f64,
    /// 今日最低价（注意：非当前 K 线的最低价，而是当日累计）
    pub low: f64,
    /// 今日累计成交量（注意：非当前 K 线的成交量）
    pub volume: f64,
    /// 昨收价
    pub prev_settle: f64,
    /// 结算价
    pub settle_price: f64,
    /// 行情时间
    pub quote_time: String,
    /// 行情日期
    pub date: String,
    /// 证券名称
    pub name: String,
    /// 本地接收时间戳
    pub timestamp: i64,
}

impl Default for Quote {
    fn default() -> Self {
        Self {
            symbol: String::new(),
            price: 0.0,
            bid_price: 0.0,
            ask_price: 0.0,
            open: 0.0,
            high: 0.0,
            low: 0.0,
            volume: 0.0,
            prev_settle: 0.0,
            settle_price: 0.0,
            quote_time: String::new(),
            date: String::new(),
            name: String::new(),
            timestamp: Utc::now().timestamp(),
        }
    }
}

/// 行情数据更新事件
#[derive(Debug, Clone)]
pub enum QuoteEvent {
    Update(Quote),
    Error(String),
    Closed,
}

/// K线数据更新事件
#[derive(Debug, Clone)]
pub enum KlineEvent {
    Update(KlineBar),
    GapFill(Vec<KlineBar>),
    Error(String),
    Closed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duration() {
        let d = Duration::minutes(5);
        assert_eq!(d.as_secs(), 300);
        assert_eq!(d.as_minutes(), 5);

        let d2 = Duration::secs(60);
        assert_eq!(d2.as_secs(), 60);
    }
}
