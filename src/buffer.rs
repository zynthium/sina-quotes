//! 环形缓冲区 - 用于 K 线数据存储

use crate::types::KlineBar;
use std::collections::VecDeque;

/// 固定容量的环形缓冲区
///
/// 相比 Vec，环形缓冲区在 push 时是 O(1) 操作，不需要整体搬迁数据
#[derive(Debug, Clone)]
pub struct RingBuffer<T> {
    buf: VecDeque<T>,
    capacity: usize,
}

impl<T> RingBuffer<T> {
    /// 创建新的环形缓冲区
    pub fn new(capacity: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// 添加新元素（满了自动覆盖最旧的）
    pub fn push(&mut self, item: T) {
        if self.buf.len() >= self.capacity {
            self.buf.pop_front();
        }
        self.buf.push_back(item);
    }

    /// 更新最后一个元素（用于实时更新当前 bar）
    pub fn update_last(&mut self, item: T) -> bool {
        if let Some(last) = self.buf.back_mut() {
            *last = item;
            true
        } else {
            false
        }
    }

    /// 获取第 i 个元素（0 = 最旧）
    pub fn get(&self, i: usize) -> Option<&T> {
        self.buf.get(i)
    }

    /// 获取第 i 个元素的可变引用
    pub fn get_mut(&mut self, i: usize) -> Option<&mut T> {
        self.buf.get_mut(i)
    }

    /// 获取最后一个元素
    pub fn last(&self) -> Option<&T> {
        self.buf.back()
    }

    /// 获取最后一个元素的可变引用
    pub fn last_mut(&mut self) -> Option<&mut T> {
        self.buf.back_mut()
    }

    /// 获取第一个元素
    pub fn first(&self) -> Option<&T> {
        self.buf.front()
    }

    /// 获取元素数量
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// 检查是否为空
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// 获取容量
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// 清空缓冲区
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// 转换为 Vec
    pub fn to_vec(&self) -> Vec<&T> {
        self.buf.iter().collect()
    }

    /// 迭代器
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.buf.iter()
    }

    /// 可变迭代器
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.buf.iter_mut()
    }

    /// 获取最近的 N 个元素
    pub fn recent(&self, n: usize) -> Vec<&T> {
        let start = self.buf.len().saturating_sub(n);
        self.buf.iter().skip(start).collect()
    }
}

impl<T> Default for RingBuffer<T> {
    fn default() -> Self {
        Self::new(100)
    }
}

// ===================== 专门用于 K线的环形缓冲区 =====================

/// K线专用的环形缓冲区
#[derive(Debug, Clone)]
pub struct KlineRingBuffer {
    inner: RingBuffer<KlineBar>,
    /// 最后一个 bar 的 ID
    last_id: i64,
}

impl KlineRingBuffer {
    /// 创建新的 K线缓冲区
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: RingBuffer::new(capacity),
            last_id: 0,
        }
    }

    /// 添加新的 K线
    pub fn push(&mut self, bar: KlineBar) {
        self.last_id = bar.id;
        self.inner.push(bar);
    }

    /// 更新当前正在形成的 K线
    pub fn update_last(&mut self, bar: KlineBar) -> bool {
        // 确保是新 bar
        if bar.id < self.last_id {
            return false;
        }
        self.last_id = bar.id;
        self.inner.update_last(bar)
    }

    /// 获取最后一个完整的 bar
    pub fn last_complete(&self) -> Option<&KlineBar> {
        // 如果缓冲区少于2个元素，直接返回最后一个
        if self.inner.len() < 2 {
            return self.inner.last();
        }

        // 获取最后两个元素
        let len = self.inner.len();
        let second_last = self.inner.get(len - 2);
        let last = self.inner.get(len - 1);

        if let (Some(prev), Some(curr)) = (second_last, last)
            && curr.id == prev.id + 1
        {
            return Some(prev);
        }

        self.inner.last()
    }

    /// 获取当前正在形成的 bar（如果有）
    pub fn current_bar(&self) -> Option<&KlineBar> {
        self.inner.last()
    }

    /// 检查是否是新 bar
    pub fn is_new_bar(&self, bar_id: i64) -> bool {
        if self.inner.is_empty() {
            return true;
        }
        bar_id > self.last_id
    }

    /// 获取最后一个 bar ID
    pub fn last_id(&self) -> i64 {
        self.last_id
    }

    /// 获取数据
    pub fn data(&self) -> Vec<&KlineBar> {
        self.inner.iter().collect()
    }

    /// 获取所有完整的 bars
    pub fn complete_bars(&self) -> Vec<&KlineBar> {
        if self.inner.len() < 2 {
            return self.inner.iter().collect();
        }

        let len = self.inner.len();
        let second_last = self.inner.get(len - 2);
        let last = self.inner.get(len - 1);

        if let (Some(prev), Some(curr)) = (second_last, last)
            && curr.id == prev.id + 1
        {
            return (0..len - 1).filter_map(|i| self.inner.get(i)).collect();
        }

        self.inner.iter().collect()
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// 获取最近的 N 个元素
    pub fn recent(&self, n: usize) -> Vec<&KlineBar> {
        self.inner.recent(n)
    }
}

impl Default for KlineRingBuffer {
    fn default() -> Self {
        Self::new(100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::KlineBar;

    fn make_bar(id: i64, close: f64) -> KlineBar {
        KlineBar {
            id,
            datetime: id * 60000000000, // 1分钟
            open: close - 1.0,
            high: close + 1.0,
            low: close - 2.0,
            close,
            volume: 1000.0,
            open_interest: 5000.0,
        }
    }

    #[test]
    fn test_ring_buffer() {
        let mut buf = RingBuffer::new(3);

        buf.push(1);
        buf.push(2);
        buf.push(3);
        assert_eq!(buf.len(), 3);

        // 满了后再添加，覆盖最旧的
        buf.push(4);
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.get(0), Some(&2));
        assert_eq!(buf.get(1), Some(&3));
        assert_eq!(buf.get(2), Some(&4));
    }

    #[test]
    fn test_update_last() {
        let mut buf = RingBuffer::new(3);
        buf.push(1);
        buf.push(2);

        buf.update_last(20);
        assert_eq!(buf.last(), Some(&20));
    }

    #[test]
    fn test_kline_buffer() {
        let mut buf = KlineRingBuffer::new(5);

        buf.push(make_bar(1, 100.0));
        buf.push(make_bar(2, 101.0));
        buf.push(make_bar(3, 102.0));

        assert_eq!(buf.last_id(), 3);
        assert_eq!(buf.len(), 3);

        buf.update_last(make_bar(3, 103.0));
        assert_eq!(buf.current_bar().unwrap().close, 103.0);
    }
}
