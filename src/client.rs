//! SinaQuotes SDK 客户端入口
//!
//! 统一的客户端入口，支持：
//! - K线序列订阅
//! - 实时行情订阅
//! - 历史数据获取（带缓存）

use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, RwLock as StdRwLock};
use std::time::{Duration as StdDuration, Instant};

use tokio::sync::broadcast;
use tokio::sync::{RwLock, watch};

use crate::data::series::KlineSeries;
use crate::data::types::{Duration, FuturesMarketHours, KlineBar, Quote};
use crate::error::{Result, SdkError};
use crate::net::history;
use crate::net::ws_service::WsConnection;
use crate::realtime_kline::{KlineAggregator, KlineEvent, RealtimeKline};
use crate::storage::cache::HistoryCache;
use crate::stream::{QuoteFeedManager, QuoteManager, QuoteStream};

fn compute_cache_window(duration: Duration, count: usize, now_ns: i64) -> (i64, i64) {
    let dur_ns = (duration.as_secs() as i64) * 1_000_000_000;
    if count == 0 || dur_ns <= 0 {
        return (0, 0);
    }

    let now_bucket_id = now_ns.div_euclid(dur_ns);
    let end_id = now_bucket_id + 1;
    let start_id = end_id - (count as i64);
    (start_id, end_id)
}

async fn buffer_quotes_until_ready<T, Fut>(
    rx: &mut broadcast::Receiver<Quote>,
    fut: Fut,
) -> (T, Vec<Quote>)
where
    Fut: std::future::Future<Output = T>,
{
    tokio::pin!(fut);
    let mut buffered = Vec::new();

    loop {
        tokio::select! {
            v = &mut fut => return (v, buffered),
            r = rx.recv() => {
                match r {
                    Ok(q) => buffered.push(q),
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => {}
                }
            }
        }
    }
}

#[derive(Clone)]
struct MarketHoursCacheEntry {
    value: FuturesMarketHours,
    fetched_at: Instant,
}

async fn fetch_market_hours_cached<F, Fut>(
    cache: &Arc<StdRwLock<HashMap<String, MarketHoursCacheEntry>>>,
    ttl: StdDuration,
    symbol: &str,
    now: Instant,
    fetcher: F,
) -> std::result::Result<FuturesMarketHours, history::Error>
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = std::result::Result<FuturesMarketHours, history::Error>>,
{
    let cache_key = history::market_hours_cache_key(symbol);

    {
        let cache_guard = cache.read().unwrap();
        if let Some(entry) = cache_guard.get(&cache_key)
            && now.saturating_duration_since(entry.fetched_at) < ttl
        {
            return Ok(entry.value.clone());
        }
    }

    let market_hours = fetcher(symbol.to_string()).await?;

    {
        let mut cache_guard = cache.write().unwrap();
        cache_guard.insert(
            cache_key,
            MarketHoursCacheEntry {
                value: market_hours.clone(),
                fetched_at: now,
            },
        );
    }

    Ok(market_hours)
}

/// 客户端配置
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// HTTP 请求超时
    pub http_timeout: StdDuration,
    /// WebSocket 重连延迟
    pub ws_reconnect_delay: StdDuration,
    /// 最大重连次数
    pub max_reconnect_attempts: u32,
    /// 缓存目录
    pub cache_dir: Option<PathBuf>,
    /// 缓存容量
    pub cache_capacity: usize,
    /// 默认 K线数据长度
    pub default_data_length: usize,
    /// 交易时间段缓存 TTL
    pub market_hours_cache_ttl: StdDuration,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            http_timeout: StdDuration::from_secs(10),
            ws_reconnect_delay: StdDuration::from_secs(2),
            max_reconnect_attempts: 5,
            cache_dir: None,
            cache_capacity: 100 * 1024 * 1024, // 100MB
            default_data_length: 100,
            market_hours_cache_ttl: StdDuration::from_secs(6 * 60 * 60),
        }
    }
}

/// 客户端构建器
pub struct ClientBuilder {
    config: ClientConfig,
}

impl ClientBuilder {
    pub fn new() -> Self {
        Self {
            config: ClientConfig::default(),
        }
    }

    pub fn http_timeout(mut self, timeout: StdDuration) -> Self {
        self.config.http_timeout = timeout;
        self
    }

    pub fn ws_reconnect_delay(mut self, delay: StdDuration) -> Self {
        self.config.ws_reconnect_delay = delay;
        self
    }

    pub fn max_reconnect_attempts(mut self, attempts: u32) -> Self {
        self.config.max_reconnect_attempts = attempts;
        self
    }

    pub fn cache_dir(mut self, path: PathBuf) -> Self {
        self.config.cache_dir = Some(path);
        self
    }

    pub fn cache_capacity(mut self, capacity: usize) -> Self {
        self.config.cache_capacity = capacity;
        self
    }

    pub fn default_data_length(mut self, length: usize) -> Self {
        self.config.default_data_length = length;
        self
    }

    pub fn market_hours_cache_ttl(mut self, ttl: StdDuration) -> Self {
        self.config.market_hours_cache_ttl = ttl;
        self
    }

    pub async fn build(self) -> Result<SinaQuotes> {
        SinaQuotes::with_config(self.config).await
    }
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// 更新事件类型
#[derive(Debug, Clone)]
pub enum UpdateEvent {
    /// K线更新
    Kline { symbol: String, bar: KlineBar },
    /// 行情更新
    Quote(Quote),
    /// 错误
    Error(String),
    /// 连接关闭
    Closed,
}

#[derive(Clone)]
struct RealtimeKlineEngine {
    series: KlineSeries,
    tx: broadcast::Sender<KlineEvent>,
}

/// SinaQuotes SDK 客户端
pub struct SinaQuotes {
    config: ClientConfig,
    /// K线序列
    series: Arc<RwLock<HashMap<String, KlineSeries>>>,
    /// 行情管理器
    quote_manager: QuoteManager,
    quote_feed_manager: QuoteFeedManager,
    realtime_kline_engines: Arc<RwLock<HashMap<String, RealtimeKlineEngine>>>,
    /// 历史数据缓存
    cache: Option<Arc<HistoryCache>>,
    /// 交易时间段缓存
    market_hours_cache: Arc<StdRwLock<HashMap<String, MarketHoursCacheEntry>>>,
    /// 更新通知
    update_tx: watch::Sender<u64>,
    update_rx: watch::Receiver<u64>,
    /// 更新计数器
    generation: u64,
    /// 关闭标志
    closed: Arc<RwLock<bool>>,
    /// WebSocket 连接（可选）
    ws_connection: Arc<RwLock<Option<WsConnection>>>,
}

impl SinaQuotes {
    /// 创建客户端（使用默认配置）
    pub async fn new() -> Result<Self> {
        Self::with_config(ClientConfig::default()).await
    }

    /// 使用自定义配置创建客户端
    pub async fn with_config(config: ClientConfig) -> Result<Self> {
        let (update_tx, update_rx) = watch::channel(0u64);

        let cache = if let Some(cache_dir) = &config.cache_dir {
            let c = HistoryCache::open(cache_dir)
                .map_err(|e| SdkError::Init(format!("缓存初始化失败: {}", e)))?;
            Some(Arc::new(c))
        } else {
            None
        };

        Ok(Self {
            config,
            series: Arc::new(RwLock::new(HashMap::new())),
            quote_manager: QuoteManager::new(),
            quote_feed_manager: QuoteFeedManager::new(),
            realtime_kline_engines: Arc::new(RwLock::new(HashMap::new())),
            cache,
            market_hours_cache: Arc::new(StdRwLock::new(HashMap::new())),
            update_tx,
            update_rx,
            generation: 0,
            closed: Arc::new(RwLock::new(false)),
            ws_connection: Arc::new(RwLock::new(None)),
        })
    }

    /// 创建构建器
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    // ===================== K线序列 =====================

    /// 获取 K线序列
    ///
    /// 自动从缓存或网络加载历史数据，然后返回序列句柄。
    pub async fn get_kline_serial(
        &self,
        symbol: &str,
        duration: Duration,
        data_length: usize,
    ) -> Result<KlineSeries> {
        self._check_closed().await?;

        let key = format!("{}_{}", symbol, duration.as_secs());

        // 检查是否已有序列
        {
            let series = self.series.read().await;
            if let Some(s) = series.get(&key) {
                return Ok(s.clone());
            }
        }

        // 加载历史数据
        let bars = self
            ._fetch_history_internal(symbol, duration, data_length)
            .await?;

        // 创建序列
        let series = KlineSeries::from_bars(symbol.to_string(), duration, bars);

        // 存储
        {
            let mut series_map = self.series.write().await;
            series_map.insert(key, series.clone());
        }

        Ok(series)
    }

    /// 获取已有的 K线序列（不自动加载）
    pub async fn get_kline_serial_if_exists(
        &self,
        symbol: &str,
        duration: Duration,
    ) -> Option<KlineSeries> {
        let key = format!("{}_{}", symbol, duration.as_secs());
        let series = self.series.read().await;
        series.get(&key).cloned()
    }

    /// 获取所有 K线序列
    pub async fn get_all_series(&self) -> Vec<KlineSeries> {
        let series = self.series.read().await;
        series.values().cloned().collect()
    }

    // ===================== 实时行情 =====================

    /// 订阅实时行情
    pub async fn subscribe_quotes(&self, symbols: &[&str]) -> Result<Vec<QuoteStream>> {
        self._check_closed().await?;

        let streams = self.quote_manager.subscribe_multiple(symbols).await;
        Ok(streams)
    }

    /// 订阅单个行情
    pub async fn subscribe_quote(&self, symbol: &str) -> Result<QuoteStream> {
        self._check_closed().await?;
        Ok(self.quote_manager.subscribe(symbol).await)
    }

    pub async fn subscribe_realtime_kline(
        &self,
        symbol: &str,
        duration: Duration,
        data_length: usize,
    ) -> Result<RealtimeKline> {
        self._check_closed().await?;

        let key = format!("{}_{}", symbol, duration.as_secs());

        {
            let engines = self.realtime_kline_engines.read().await;
            if let Some(engine) = engines.get(&key) {
                let rx = engine.tx.subscribe();
                if let Some(bar) = engine.series.current() {
                    let _ = engine.tx.send(KlineEvent {
                        bar,
                        is_completed: false,
                    });
                }
                return Ok(RealtimeKline::new(engine.series.clone(), rx));
            }
        }

        let mut feed_rx = self.quote_feed_manager.subscribe(symbol).await;
        let (series, buffered_quotes) =
            match self.get_kline_serial_if_exists(symbol, duration).await {
                Some(s) => (s, Vec::new()),
                None => {
                    let fut = self.get_kline_serial(symbol, duration, data_length);
                    let (res, buffered) = buffer_quotes_until_ready(&mut feed_rx, fut).await;
                    (res?, buffered)
                }
            };

        let (tx, rx) = broadcast::channel(1024);
        let engine = RealtimeKlineEngine {
            series: series.clone(),
            tx: tx.clone(),
        };

        {
            let mut engines = self.realtime_kline_engines.write().await;
            engines.insert(key.clone(), engine);
        }

        if let Some(bar) = series.current() {
            let _ = tx.send(KlineEvent {
                bar,
                is_completed: false,
            });
        }

        let cache = self.cache.clone();
        let cache_key = crate::storage::cache::CacheKey::new(symbol, duration);
        let series_for_task = series.clone();

        tokio::spawn(async move {
            let mut agg = KlineAggregator::new(duration);

            if let Some(bar) = series_for_task.current() {
                agg.seed_from_bar(bar);
            }

            for q in buffered_quotes {
                let events = agg.on_quote(q);
                for ev in events {
                    series_for_task.handle_update(ev.bar.clone());
                    let _ = tx.send(ev.clone());
                }
            }

            loop {
                let quote = match feed_rx.recv().await {
                    Ok(q) => q,
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                };

                let events = agg.on_quote(quote);
                for ev in events {
                    series_for_task.handle_update(ev.bar.clone());
                    let _ = tx.send(ev.clone());

                    if ev.is_completed
                        && let Some(cache) = cache.as_ref()
                    {
                        let cache = Arc::clone(cache);
                        let cache_key = cache_key.clone();
                        let bar = ev.bar.clone();
                        let _ = tokio::task::spawn_blocking(move || cache.put(&cache_key, &[bar]))
                            .await;
                    }
                }
            }
        });

        Ok(RealtimeKline::new(series, rx))
    }

    /// 更新行情（内部使用）
    pub async fn _update_quote(&mut self, quote: Quote) {
        self.quote_manager.update(quote.clone()).await;

        // 更新 generation
        self.generation += 1;
        let _ = self.update_tx.send(self.generation);
    }

    // ===================== 历史数据 =====================

    /// 获取历史数据
    pub async fn fetch_history(
        &self,
        symbol: &str,
        duration: Duration,
        count: usize,
    ) -> Result<Vec<KlineBar>> {
        self._check_closed().await?;
        self._fetch_history_internal(symbol, duration, count).await
    }

    /// 获取期货交易时间段
    pub async fn fetch_market_hours(&self, symbol: &str) -> Result<FuturesMarketHours> {
        self._check_closed().await?;
        fetch_market_hours_cached(
            &self.market_hours_cache,
            self.config.market_hours_cache_ttl,
            symbol,
            Instant::now(),
            |symbol| async move { history::fetch_market_hours(&symbol, 3).await },
        )
        .await
        .map_err(|e| match e {
            history::Error::Request(e) => SdkError::Http(e),
            history::Error::Parse(s) => SdkError::Parse(s),
            history::Error::Empty => SdkError::MarketHoursUnavailable {
                category: infer_market_hours_category(symbol).to_string(),
                symbol: symbol.to_string(),
            },
        })
    }

    /// 内部：获取历史数据
    async fn _fetch_history_internal(
        &self,
        symbol: &str,
        duration: Duration,
        count: usize,
    ) -> Result<Vec<KlineBar>> {
        // 1. 尝试从缓存读取
        if let Some(cache) = &self.cache {
            let cache_key = crate::storage::cache::CacheKey::new(symbol, duration);
            let now_ns = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
            let (start_id, end_id) = compute_cache_window(duration, count, now_ns);
            let cached = cache.get(&cache_key, start_id, end_id);

            if cached.len() >= count {
                tracing::debug!("缓存命中: {} {} bars", symbol, cached.len());
                return Ok(cached);
            }
        }

        // 2. 从网络获取
        let duration_seconds = duration.as_secs() as u32;
        let bars = history::fetch_history(symbol, duration_seconds, 3)
            .await
            .map_err(|e| match e {
                history::Error::Request(e) => SdkError::Http(e),
                history::Error::Parse(s) => SdkError::Parse(s),
                history::Error::Empty => SdkError::HistoryUnavailable(symbol.to_string()),
            })?;

        // 3. 存入缓存
        if let Some(cache) = &self.cache {
            let cache_key = crate::storage::cache::CacheKey::new(symbol, duration);
            // 需要转换回可序列化的格式
            let cache_bars: Vec<KlineBar> = bars.to_vec();
            if let Err(e) = cache.put(&cache_key, &cache_bars) {
                tracing::warn!("缓存写入失败: {}", e);
            }
        }

        // 4. 限制数量
        let result: Vec<KlineBar> = bars.into_iter().take(count).collect();
        Ok(result)
    }

    /// 预热缓存
    pub async fn warm_cache(
        &self,
        symbol: &str,
        duration: Duration,
        start_id: i64,
        end_id: i64,
    ) -> Result<()> {
        self._check_closed().await?;

        let Some(cache) = &self.cache else {
            return Ok(());
        };

        let cache_key = crate::storage::cache::CacheKey::new(symbol, duration);

        // 获取缺失的范围
        let missing = cache.missing_ranges(&cache_key, start_id, end_id);

        if missing.is_empty() {
            tracing::debug!("缓存已完整: {}", symbol);
            return Ok(());
        }

        tracing::info!("预热缓存: {} 需要下载 {} 段", symbol, missing.len());

        // 下载每段数据
        for (start, _end) in missing {
            let duration_seconds = duration.as_secs() as u32;

            match history::fetch_history(symbol, duration_seconds, 3).await {
                Ok(mut bars) => {
                    for bar in &mut bars {
                        bar.id = start + bar.id - 1;
                    }
                    if let Err(e) = cache.put(&cache_key, &bars) {
                        tracing::warn!("缓存写入失败: {}", e);
                    }
                }
                Err(e) => {
                    tracing::warn!("预热下载失败: {:?}", e);
                }
            }
        }

        Ok(())
    }

    // ===================== 事件循环 =====================

    /// 等待数据更新
    ///
    /// 类似于 TqSdk 的 wait_update()，阻塞直到有数据更新。
    pub async fn wait_update(&mut self) -> Result<()> {
        self._check_closed().await?;

        self.update_rx.changed().await.map_err(|_| SdkError::Closed)
    }

    /// 获取更新计数
    pub fn generation(&self) -> u64 {
        self.generation
    }

    // ===================== 缓存管理 =====================

    /// 获取缓存统计
    pub fn cache_stats(&self) -> Option<crate::storage::cache::CacheStats> {
        self.cache.as_ref().map(|c| c.stats())
    }

    /// 清除缓存
    pub fn clear_cache(&self) -> Result<()> {
        if let Some(cache) = &self.cache {
            cache
                .clear_all()
                .map_err(|e| SdkError::Init(e.to_string()))?;
        }
        self.market_hours_cache.write().unwrap().clear();
        Ok(())
    }

    // ===================== 生命周期 =====================

    /// 检查是否已关闭
    pub async fn is_closed(&self) -> bool {
        *self.closed.read().await
    }

    /// 关闭客户端
    pub async fn close(self) {
        let mut closed = self.closed.write().await;
        *closed = true;

        // 清空序列
        let mut series = self.series.write().await;
        series.clear();

        // 清空行情
        self.quote_manager.clear().await;

        // 关闭 WebSocket 连接
        let mut ws_conn = self.ws_connection.write().await;
        if let Some(conn) = ws_conn.take() {
            conn.close().await;
        }

        tracing::info!("SinaQuotes 客户端已关闭");
    }

    // ===================== WebSocket 连接 =====================

    /// 启动 WebSocket 连接
    ///
    /// 在后台线程中运行，自动重连。
    /// 返回是否成功启动。
    pub async fn start_websocket(&self, symbols: Vec<String>) -> Result<()> {
        self._check_closed().await?;

        let mut ws_conn = self.ws_connection.write().await;
        if ws_conn.is_some() {
            return Err(SdkError::Init("WebSocket 连接已存在".to_string()));
        }

        let conn = WsConnection::new(
            symbols,
            self.quote_manager.clone(),
            self.quote_feed_manager.clone(),
            self.config.ws_reconnect_delay,
            self.config.max_reconnect_attempts,
        );

        let conn_clone = conn.clone();
        tokio::spawn(async move {
            conn_clone.start().await;
        });

        *ws_conn = Some(conn);
        Ok(())
    }

    /// 停止 WebSocket 连接
    pub async fn stop_websocket(&self) -> Result<()> {
        self._check_closed().await?;

        let mut ws_conn = self.ws_connection.write().await;
        if let Some(conn) = ws_conn.take() {
            conn.close().await;
        }
        Ok(())
    }

    /// 获取 WebSocket 连接状态
    pub async fn ws_state(&self) -> Option<crate::net::ws_service::WsState> {
        let ws_conn = self.ws_connection.read().await;
        if let Some(conn) = ws_conn.as_ref() {
            Some(conn.state().await)
        } else {
            None
        }
    }

    async fn _check_closed(&self) -> Result<()> {
        if *self.closed.read().await {
            return Err(SdkError::Closed);
        }
        Ok(())
    }
}

fn infer_market_hours_category(symbol: &str) -> &'static str {
    if symbol.trim().starts_with("hf_") {
        "hf"
    } else {
        "nf"
    }
}

impl Clone for SinaQuotes {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            series: Arc::clone(&self.series),
            quote_manager: self.quote_manager.clone(),
            quote_feed_manager: self.quote_feed_manager.clone(),
            realtime_kline_engines: Arc::clone(&self.realtime_kline_engines),
            cache: self.cache.clone(),
            market_hours_cache: Arc::clone(&self.market_hours_cache),
            update_tx: self.update_tx.clone(),
            update_rx: self.update_rx.clone(),
            generation: self.generation,
            closed: Arc::clone(&self.closed),
            ws_connection: Arc::clone(&self.ws_connection),
        }
    }
}

impl std::fmt::Debug for SinaQuotes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SinaQuotes")
            .field("config", &self.config)
            .field("closed", &self.closed)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::series::KlineSeries;
    use crate::data::types::{FuturesCategory, TradingSession};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;

    fn make_market_hours(symbol: &str, timezone: &str) -> FuturesMarketHours {
        FuturesMarketHours {
            category: FuturesCategory::Nf,
            symbol: symbol.to_string(),
            timezone: timezone.to_string(),
            exchange: None,
            interval: None,
            sessions: vec![TradingSession {
                start: "09:00".to_string(),
                end: "15:00".to_string(),
            }],
        }
    }

    #[tokio::test]
    async fn test_client_creation() {
        let client = SinaQuotes::new().await;
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_builder() {
        let client = SinaQuotes::builder()
            .http_timeout(StdDuration::from_secs(5))
            .default_data_length(200)
            .market_hours_cache_ttl(StdDuration::from_secs(42))
            .build()
            .await;

        assert!(client.is_ok());
        let c = client.unwrap();
        assert_eq!(c.config.default_data_length, 200);
        assert_eq!(c.config.market_hours_cache_ttl, StdDuration::from_secs(42));
    }

    #[test]
    fn test_compute_cache_window_ids() {
        let duration = Duration::minutes(1);
        let dur_ns = (duration.as_secs() as i64) * 1_000_000_000;
        let now_ns = 100 * dur_ns + 123;
        let (start_id, end_id) = compute_cache_window(duration, 3, now_ns);
        assert_eq!(start_id, 98);
        assert_eq!(end_id, 101);
    }

    #[tokio::test]
    async fn test_fetch_market_hours_cached_hits_cache_within_ttl() {
        let cache = Arc::new(StdRwLock::new(
            HashMap::<String, MarketHoursCacheEntry>::new(),
        ));
        let fetch_count = Arc::new(AtomicUsize::new(0));
        let now = Instant::now();

        let first = fetch_market_hours_cached(&cache, StdDuration::from_secs(300), "SC0", now, {
            let fetch_count = Arc::clone(&fetch_count);
            move |symbol| {
                let fetch_count = Arc::clone(&fetch_count);
                async move {
                    fetch_count.fetch_add(1, Ordering::SeqCst);
                    Ok(make_market_hours(&symbol, "summer"))
                }
            }
        })
        .await
        .unwrap();

        let second = fetch_market_hours_cached(
            &cache,
            StdDuration::from_secs(300),
            "nf_SC0",
            now + StdDuration::from_secs(60),
            {
                let fetch_count = Arc::clone(&fetch_count);
                move |symbol| {
                    let fetch_count = Arc::clone(&fetch_count);
                    async move {
                        fetch_count.fetch_add(1, Ordering::SeqCst);
                        Ok(make_market_hours(&symbol, "winter"))
                    }
                }
            },
        )
        .await
        .unwrap();

        assert_eq!(fetch_count.load(Ordering::SeqCst), 1);
        assert_eq!(first.timezone, "summer");
        assert_eq!(second.timezone, "summer");
        assert_eq!(second.symbol, "SC0");
    }

    #[tokio::test]
    async fn test_fetch_market_hours_cached_refetches_after_ttl() {
        let cache = Arc::new(StdRwLock::new(
            HashMap::<String, MarketHoursCacheEntry>::new(),
        ));
        let fetch_count = Arc::new(AtomicUsize::new(0));
        let now = Instant::now();

        let first = fetch_market_hours_cached(&cache, StdDuration::from_secs(300), "SC0", now, {
            let fetch_count = Arc::clone(&fetch_count);
            move |symbol| {
                let fetch_count = Arc::clone(&fetch_count);
                async move {
                    fetch_count.fetch_add(1, Ordering::SeqCst);
                    Ok(make_market_hours(&symbol, "summer"))
                }
            }
        })
        .await
        .unwrap();

        let second = fetch_market_hours_cached(
            &cache,
            StdDuration::from_secs(300),
            "SC0",
            now + StdDuration::from_secs(301),
            {
                let fetch_count = Arc::clone(&fetch_count);
                move |symbol| {
                    let fetch_count = Arc::clone(&fetch_count);
                    async move {
                        fetch_count.fetch_add(1, Ordering::SeqCst);
                        Ok(make_market_hours(&symbol, "winter"))
                    }
                }
            },
        )
        .await
        .unwrap();

        assert_eq!(fetch_count.load(Ordering::SeqCst), 2);
        assert_eq!(first.timezone, "summer");
        assert_eq!(second.timezone, "winter");
    }

    #[tokio::test]
    async fn test_subscribe_realtime_kline_emits_events_from_quote_feed() {
        let client = SinaQuotes::new().await.unwrap();

        let symbol = "hf_TEST";
        let duration = Duration::minutes(1);
        let key = format!("{}_{}", symbol, duration.as_secs());

        let series = KlineSeries::new(symbol.to_string(), duration, 10);
        {
            let mut map = client.series.write().await;
            map.insert(key, series.clone());
        }

        let mut sub = client
            .subscribe_realtime_kline(symbol, duration, 10)
            .await
            .unwrap();

        client
            .quote_feed_manager
            .publish(Quote {
                symbol: symbol.to_string(),
                price: 10.0,
                volume: 100.0,
                timestamp: 60,
                ..Default::default()
            })
            .await;

        let ev = tokio::time::timeout(StdDuration::from_secs(1), sub.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ev.bar.close, 10.0);
        assert_eq!(sub.series().symbol(), symbol);
    }

    #[tokio::test]
    async fn test_buffer_quotes_until_ready_captures_quotes() {
        let (tx, mut rx) = broadcast::channel(16);

        tokio::spawn(async move {
            tokio::time::sleep(StdDuration::from_millis(10)).await;
            let _ = tx.send(Quote {
                symbol: "hf_TEST".to_string(),
                price: 1.0,
                volume: 1.0,
                timestamp: 1,
                ..Default::default()
            });
        });

        let fut = async {
            tokio::time::sleep(StdDuration::from_millis(30)).await;
            7u32
        };

        let (v, buffered) = buffer_quotes_until_ready(&mut rx, fut).await;
        assert_eq!(v, 7);
        assert_eq!(buffered.len(), 1);
        assert_eq!(buffered[0].price, 1.0);
    }

    #[tokio::test]
    async fn test_subscribe_realtime_kline_emits_current_bar_immediately() {
        let client = SinaQuotes::new().await.unwrap();

        let symbol = "hf_TEST";
        let duration = Duration::minutes(1);
        let key = format!("{}_{}", symbol, duration.as_secs());

        let series = KlineSeries::new(symbol.to_string(), duration, 10);
        series.push(KlineBar {
            id: 1,
            datetime: 0,
            open: 1.0,
            high: 1.0,
            low: 1.0,
            close: 5.0,
            volume: 0.0,
            open_interest: 0.0,
        });
        {
            let mut map = client.series.write().await;
            map.insert(key, series.clone());
        }

        let mut sub = client
            .subscribe_realtime_kline(symbol, duration, 10)
            .await
            .unwrap();

        let ev = tokio::time::timeout(StdDuration::from_secs(1), sub.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ev.bar.close, 5.0);
        assert!(!ev.is_completed);
    }
}
