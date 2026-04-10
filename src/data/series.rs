//! K线序列 - 核心数据类型，类似 TqSdk 的 get_kline_serial()
//!
//! KlineSeries 是用户持有的数据序列句柄，内部使用 `Arc<RwLock>` 实现
//! 零拷贝读取，同时支持 SDK 内部更新。

use std::sync::{Arc, RwLock};

use crate::data::buffer::KlineRingBuffer;
use crate::data::types::{Duration, KlineBar, KlineData};

/// K线序列句柄
///
/// 用户持有此句柄，在 `wait_update()` 返回后可以安全读取数据。
/// 数据更新通过 `Arc<RwLock>` 实现，支持多个读者同时读取。
pub struct KlineSeries {
    inner: Arc<RwLock<KlineSeriesInner>>,
}

/// 内部实现
pub(crate) struct KlineSeriesInner {
    /// 符号
    symbol: String,
    /// 周期
    duration: Duration,
    /// 环形缓冲区
    buffer: KlineRingBuffer,
    /// 容量
    capacity: usize,
}

impl KlineSeries {
    /// 创建新的 K线序列
    pub fn new(symbol: String, duration: Duration, capacity: usize) -> Self {
        Self {
            inner: Arc::new(RwLock::new(KlineSeriesInner {
                symbol,
                duration,
                buffer: KlineRingBuffer::new(capacity),
                capacity,
            })),
        }
    }

    /// 从已有数据创建
    pub fn from_bars(symbol: String, duration: Duration, bars: Vec<KlineBar>) -> Self {
        let capacity = bars.len().max(100);
        let series = Self::new(symbol, duration, capacity);

        for bar in bars {
            series.push(bar);
        }

        series
    }

    /// 读取所有完整 K线（不包括正在形成的）
    pub fn read(&self) -> Vec<KlineBar> {
        let inner = self.inner.read().unwrap();
        inner.buffer.complete_bars().into_iter().cloned().collect()
    }

    /// 获取所有数据（包括正在形成的）
    pub fn read_all(&self) -> Vec<KlineBar> {
        let inner = self.inner.read().unwrap();
        inner.buffer.data().into_iter().cloned().collect()
    }

    /// 获取最后一个完整的 K线
    pub fn last(&self) -> Option<KlineBar> {
        let inner = self.inner.read().unwrap();
        inner.buffer.last_complete().cloned()
    }

    /// 获取当前正在形成的 K线（如果有）
    pub fn current(&self) -> Option<KlineBar> {
        let inner = self.inner.read().unwrap();
        inner.buffer.current_bar().cloned()
    }

    /// 获取第 i 个 K线（0 = 最旧）
    pub fn get(&self, i: usize) -> Option<KlineBar> {
        let inner = self.inner.read().unwrap();
        let data = inner.buffer.data();
        data.get(i).map(|b| (*b).clone())
    }

    /// 获取最近的 N 个 K线
    pub fn recent(&self, n: usize) -> Vec<KlineBar> {
        let inner = self.inner.read().unwrap();
        inner.buffer.recent(n).into_iter().cloned().collect()
    }

    /// 获取数据长度
    pub fn len(&self) -> usize {
        let inner = self.inner.read().unwrap();
        inner.buffer.data().len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 获取容量
    pub fn capacity(&self) -> usize {
        let inner = self.inner.read().unwrap();
        inner.capacity
    }

    /// 获取符号
    pub fn symbol(&self) -> String {
        let inner = self.inner.read().unwrap();
        inner.symbol.clone()
    }

    /// 获取周期
    pub fn duration(&self) -> Duration {
        let inner = self.inner.read().unwrap();
        inner.duration
    }

    /// 获取最后一个 bar ID
    pub fn last_id(&self) -> i64 {
        let inner = self.inner.read().unwrap();
        inner.buffer.last_id()
    }

    /// 推送新的 K线
    pub fn push(&self, bar: KlineBar) {
        let mut inner = self.inner.write().unwrap();
        inner.buffer.push(bar);
    }

    /// 更新当前 K线
    pub fn update_last(&self, bar: KlineBar) -> bool {
        let mut inner = self.inner.write().unwrap();
        inner.buffer.update_last(bar)
    }

    /// 处理更新事件（自动判断是新 bar 还是更新）
    pub fn handle_update(&self, bar: KlineBar) {
        let mut inner = self.inner.write().unwrap();

        if inner.buffer.is_new_bar(bar.id) {
            inner.buffer.push(bar);
        } else {
            inner.buffer.update_last(bar);
        }
    }

    /// 填充缺失的 K线
    pub fn fill_gap(&self, bars: Vec<KlineBar>) {
        let mut inner = self.inner.write().unwrap();
        for bar in bars {
            inner.buffer.push(bar);
        }
    }

    /// 清空数据
    pub fn clear(&self) {
        let mut inner = self.inner.write().unwrap();
        inner.buffer = KlineRingBuffer::new(inner.capacity);
    }
}

impl Clone for KlineSeries {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl std::fmt::Debug for KlineSeries {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.read().unwrap();
        f.debug_struct("KlineSeries")
            .field("symbol", &inner.symbol)
            .field("duration", &inner.duration)
            .field("len", &inner.buffer.data().len())
            .field("capacity", &inner.capacity)
            .finish()
    }
}

// ===================== Trait 实现 =====================

impl KlineSeriesInner {
    /// 转换为 KlineData
    pub fn to_data(&self) -> KlineData {
        KlineData {
            symbol: self.symbol.clone(),
            duration: self.duration,
            bars: self.buffer.data().into_iter().cloned().collect(),
        }
    }
}

/// KlineSeries 转换为 KlineData
impl From<&KlineSeries> for KlineData {
    fn from(series: &KlineSeries) -> Self {
        let inner = series.inner.read().unwrap();
        inner.to_data()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_bar(id: i64, close: f64) -> KlineBar {
        KlineBar {
            id,
            datetime: id * 60000000000,
            open: close - 1.0,
            high: close + 1.0,
            low: close - 2.0,
            close,
            volume: 1000.0,
            open_interest: 5000.0,
        }
    }

    #[test]
    fn test_kline_series_basic() {
        let series = KlineSeries::new("hf_CL".to_string(), Duration::minutes(5), 100);

        series.push(make_bar(1, 100.0));
        series.push(make_bar(2, 101.0));
        series.push(make_bar(3, 102.0));

        assert_eq!(series.len(), 3);
        // last_complete() excludes the "current" bar (last one)
        // so with 3 bars, it returns the 2nd bar (the last complete one)
        assert_eq!(series.last().unwrap().close, 101.0);
        // current() returns the most recent bar (even if incomplete)
        assert_eq!(series.current().unwrap().close, 102.0);
    }

    #[test]
    fn test_update_current_bar() {
        let series = KlineSeries::new("hf_CL".to_string(), Duration::minutes(5), 100);

        series.push(make_bar(1, 100.0));
        series.push(make_bar(2, 101.0));

        // 更新当前 bar (bar id=2)
        series.update_last(make_bar(2, 102.0));

        // last_complete() returns the last complete bar (id=1)
        assert_eq!(series.last().unwrap().close, 100.0);
        // current() returns the current bar being formed
        assert_eq!(series.current().unwrap().close, 102.0);
    }

    #[test]
    fn test_clone_share_data() {
        let series1 = KlineSeries::new("hf_CL".to_string(), Duration::minutes(5), 100);
        let series2 = series1.clone();

        series1.push(make_bar(1, 100.0));

        // 两个引用应该看到相同的数据
        assert_eq!(series2.len(), 1);
    }

    #[test]
    fn test_recent() {
        let series = KlineSeries::new("hf_CL".to_string(), Duration::minutes(5), 10);

        for i in 1..=10 {
            series.push(make_bar(i, 100.0 + i as f64));
        }

        let recent = series.recent(3);
        assert_eq!(recent.len(), 3);
        assert_eq!(recent[0].id, 8);
        assert_eq!(recent[2].id, 10);
    }
}
