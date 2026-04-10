//! QuoteStream - 实时行情流
//!
//! 使用 tokio watch 通道实现"一个写者、多个读者"的模式。

use std::sync::Arc;
use tokio::sync::{RwLock, broadcast, watch};

use crate::data::types::Quote;

/// 实时行情流
///
/// 用户持有此句柄，可以随时获取最新行情数据。
/// 内部使用 watch 通道，支持多个读者同时读取最新值。
pub struct QuoteStream {
    rx: watch::Receiver<Quote>,
}

impl QuoteStream {
    /// 创建新的行情流
    pub fn new(rx: watch::Receiver<Quote>) -> Self {
        Self { rx }
    }

    /// 获取当前行情（零拷贝）
    pub fn borrow(&self) -> watch::Ref<'_, Quote> {
        self.rx.borrow()
    }

    /// 获取当前行情的克隆
    pub fn get(&self) -> Quote {
        self.rx.borrow().clone()
    }

    /// 获取符号
    pub fn symbol(&self) -> String {
        self.rx.borrow().symbol.clone()
    }

    /// 检查是否有值
    pub fn has_value(&self) -> bool {
        self.rx.has_changed().unwrap_or(false) || self.rx.borrow().symbol.is_empty()
    }

    /// 等待行情更新
    pub async fn changed(&mut self) -> Result<(), watch::error::RecvError> {
        self.rx.changed().await
    }

    /// 获取引用
    pub fn inner(&self) -> &watch::Receiver<Quote> {
        &self.rx
    }
}

impl Clone for QuoteStream {
    fn clone(&self) -> Self {
        Self {
            rx: self.rx.clone(),
        }
    }
}

impl std::fmt::Debug for QuoteStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuoteStream")
            .field("symbol", &self.borrow().symbol)
            .finish()
    }
}

// ===================== 多行情管理 =====================

use std::collections::HashMap;

/// 多行情流管理器
pub struct QuoteManager {
    /// 符号 -> watch::Sender
    streams: Arc<RwLock<HashMap<String, watch::Sender<Quote>>>>,
}

pub struct QuoteFeedManager {
    streams: Arc<RwLock<HashMap<String, broadcast::Sender<Quote>>>>,
}

impl QuoteFeedManager {
    pub fn new() -> Self {
        Self {
            streams: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn subscribe(&self, symbol: &str) -> broadcast::Receiver<Quote> {
        let mut streams = self.streams.write().await;
        let tx = streams.entry(symbol.to_string()).or_insert_with(|| {
            let (tx, _) = broadcast::channel(1024);
            tx
        });
        tx.subscribe()
    }

    pub async fn publish(&self, quote: Quote) {
        let tx = {
            let mut streams = self.streams.write().await;
            streams
                .entry(quote.symbol.clone())
                .or_insert_with(|| {
                    let (tx, _) = broadcast::channel(1024);
                    tx
                })
                .clone()
        };
        let _ = tx.send(quote);
    }
}

impl Default for QuoteFeedManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for QuoteFeedManager {
    fn clone(&self) -> Self {
        Self {
            streams: Arc::clone(&self.streams),
        }
    }
}

impl QuoteManager {
    /// 创建新的管理器
    pub fn new() -> Self {
        Self {
            streams: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 订阅行情，返回流句柄
    pub async fn subscribe(&self, symbol: &str) -> QuoteStream {
        let mut streams = self.streams.write().await;

        let tx = streams.entry(symbol.to_string()).or_insert_with(|| {
            let (tx, _) = watch::channel(Quote::default());
            tx
        });

        let stream = QuoteStream::new(tx.subscribe());

        // 发送初始 Quote 以设置符号
        let initial = Quote {
            symbol: symbol.to_string(),
            ..Default::default()
        };
        let _ = tx.send(initial);

        stream
    }

    /// 订阅多个行情
    pub async fn subscribe_multiple(&self, symbols: &[&str]) -> Vec<QuoteStream> {
        let mut streams = Vec::with_capacity(symbols.len());

        for &symbol in symbols {
            streams.push(self.subscribe(symbol).await);
        }

        streams
    }

    /// 更新行情
    pub async fn update(&self, quote: Quote) {
        let streams = self.streams.read().await;

        if let Some(tx) = streams.get(&quote.symbol) {
            let _ = tx.send(quote);
        }
    }

    /// 获取所有符号
    pub async fn symbols(&self) -> Vec<String> {
        let streams = self.streams.read().await;
        streams.keys().cloned().collect()
    }

    /// 取消订阅
    pub async fn unsubscribe(&self, symbol: &str) {
        let mut streams = self.streams.write().await;
        streams.remove(symbol);
    }

    /// 清空所有订阅
    pub async fn clear(&self) {
        let mut streams = self.streams.write().await;
        streams.clear();
    }
}

impl Default for QuoteManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for QuoteManager {
    fn clone(&self) -> Self {
        Self {
            streams: Arc::clone(&self.streams),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::watch;

    #[tokio::test]
    async fn test_quote_stream() {
        let (tx, rx) = watch::channel(Quote::default());
        let stream = QuoteStream::new(rx);

        let quote = Quote {
            symbol: "hf_CL".to_string(),
            price: 100.0,
            ..Default::default()
        };

        tx.send(quote).unwrap();

        // 通过 borrow 获取
        let borrowed = stream.borrow();
        assert_eq!(borrowed.symbol, "hf_CL");
        assert_eq!(borrowed.price, 100.0);
    }

    #[tokio::test]
    async fn test_quote_manager() {
        let manager = QuoteManager::new();

        let stream1 = manager.subscribe("hf_CL").await;
        let stream2 = manager.subscribe("hf_GC").await;

        assert_eq!(stream1.symbol(), "hf_CL");
        assert_eq!(stream2.symbol(), "hf_GC");

        manager
            .update(Quote {
                symbol: "hf_CL".to_string(),
                price: 100.0,
                ..Default::default()
            })
            .await;

        // stream1 应该能看到更新
        let quote = stream1.borrow();
        assert_eq!(quote.price, 100.0);
    }

    #[tokio::test]
    async fn test_multiple_readers() {
        let (tx, rx) = watch::channel(Quote::default());
        let stream1 = QuoteStream::new(rx);
        let stream2 = stream1.clone();

        let quote = Quote {
            symbol: "hf_CL".to_string(),
            price: 100.0,
            ..Default::default()
        };

        tx.send(quote).unwrap();

        assert_eq!(stream1.borrow().price, 100.0);
        assert_eq!(stream2.borrow().price, 100.0);
    }

    #[tokio::test]
    async fn test_quote_feed_receives_all_updates() {
        let feed = QuoteFeedManager::new();
        let mut rx = feed.subscribe("hf_TEST").await;

        feed.publish(Quote {
            symbol: "hf_TEST".to_string(),
            price: 1.0,
            ..Default::default()
        })
        .await;
        feed.publish(Quote {
            symbol: "hf_TEST".to_string(),
            price: 2.0,
            ..Default::default()
        })
        .await;

        let q1 = rx.recv().await.unwrap();
        let q2 = rx.recv().await.unwrap();
        assert_eq!(q1.price, 1.0);
        assert_eq!(q2.price, 2.0);
    }
}
