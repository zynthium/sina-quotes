//! 历史数据缓存管理器。
//!
//! 仅使用新的段缓存实现，不再兼容旧版 `.bin + index.json` 缓存格式。

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::data::types::{Duration, KlineBar, Quote};
use crate::error::Result;
use crate::storage::data_series::DataSeriesCache;
use crate::storage::rangeset::{Range, RangeSet};

/// 缓存键
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub symbol: String,
    pub duration: Duration,
}

impl CacheKey {
    pub fn new(symbol: &str, duration: Duration) -> Self {
        Self {
            symbol: symbol.to_string(),
            duration,
        }
    }

    /// 保留旧 API 形状，但底层已不再使用该命名方案。
    pub fn cache_filename(&self) -> String {
        format!("{}_{}.bin", self.symbol, self.duration.as_secs())
    }
}

/// 缓存条目元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntryMeta {
    pub symbol: String,
    pub duration: Duration,
    pub ranges: RangeSet,
    pub updated_at: u64,
    pub bar_count: usize,
}

/// 旧版缓存索引结构。
///
/// 该类型保留为公共 API 的一部分，但新的缓存实现不会再读写它。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheIndex {
    pub version: u32,
    pub entries: HashMap<String, CacheEntryMeta>,
}

impl CacheIndex {
    pub fn new() -> Self {
        Self {
            version: 1,
            entries: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PendingKey {
    symbol: String,
    duration_ns: i64,
}

impl PendingKey {
    fn new(symbol: &str, duration_ns: i64) -> Self {
        Self {
            symbol: symbol.to_string(),
            duration_ns,
        }
    }
}

/// 历史数据缓存
pub struct HistoryCache {
    cache_dir: PathBuf,
    series_cache: DataSeriesCache,
    pending: Mutex<HashMap<PendingKey, KlineBar>>,
    max_total_bytes: Option<u64>,
}

impl HistoryCache {
    /// 创建或打开缓存
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        Self::open_with_capacity(path, None)
    }

    pub fn open_with_capacity(
        path: impl Into<PathBuf>,
        max_total_bytes: Option<u64>,
    ) -> Result<Self> {
        let cache_dir = path.into();
        fs::create_dir_all(&cache_dir)?;

        Ok(Self {
            series_cache: DataSeriesCache::new(cache_dir.join("series_v2"))?,
            pending: Mutex::new(HashMap::new()),
            max_total_bytes,
            cache_dir,
        })
    }

    /// 获取缓存目录
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    fn series_duration_ns(key: &CacheKey) -> i64 {
        i64::try_from(key.duration.ns).unwrap_or_default()
    }

    fn all_rows(&self, key: &CacheKey) -> Result<Vec<KlineBar>> {
        let duration_ns = Self::series_duration_ns(key);
        if duration_ns <= 0 {
            return Ok(Vec::new());
        }

        let ranges = self
            .series_cache
            .get_rangeset_id(&key.symbol, duration_ns)?;
        let Some((start_id, _)) = ranges.first().copied() else {
            return Ok(Vec::new());
        };
        let end_id = ranges.last().map(|range| range.1).unwrap_or(start_id);

        self.series_cache
            .read_kline_window_by_id(&key.symbol, duration_ns, start_id, end_id)
    }

    fn pending_bar(&self, key: &CacheKey) -> Option<KlineBar> {
        let duration_ns = Self::series_duration_ns(key);
        let pending = self.pending.lock().ok()?;
        pending
            .get(&PendingKey::new(&key.symbol, duration_ns))
            .cloned()
    }

    fn overlay_pending(&self, key: &CacheKey, rows: Vec<KlineBar>) -> Vec<KlineBar> {
        let Some(pending_bar) = self.pending_bar(key) else {
            return rows;
        };

        let mut merged = BTreeMap::new();
        for row in rows {
            merged.insert(row.id, row);
        }
        merged.insert(pending_bar.id, pending_bar);
        merged.into_values().collect()
    }

    fn tail_rows(rows: Vec<KlineBar>, count: usize) -> Vec<KlineBar> {
        let skip = rows.len().saturating_sub(count);
        rows.into_iter().skip(skip).collect()
    }

    fn build_bar(duration_ns: i64, bucket_id: i64, quote: &Quote, delta_volume: f64) -> KlineBar {
        KlineBar {
            id: bucket_id,
            datetime: bucket_id * duration_ns,
            open: quote.price,
            high: quote.price,
            low: quote.price,
            close: quote.price,
            volume: delta_volume,
            open_interest: 0.0,
        }
    }

    fn update_bar(bar: &mut KlineBar, quote: &Quote, delta_volume: f64) {
        bar.high = bar.high.max(quote.price);
        bar.low = bar.low.min(quote.price);
        bar.close = quote.price;
        bar.volume += delta_volume;
    }

    fn enforce_limits(&self) -> Result<()> {
        self.series_cache.enforce_limits(self.max_total_bytes)
    }

    pub(crate) fn has_pending_symbol(&self, symbol: &str) -> bool {
        let Ok(pending) = self.pending.lock() else {
            return false;
        };
        pending.keys().any(|key| key.symbol == symbol)
    }

    fn flush_pending_entries(&self, entries: Vec<(PendingKey, KlineBar)>) -> Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        for (key, bar) in entries {
            self.series_cache
                .upsert_last_kline_row(&key.symbol, key.duration_ns, &bar)?;
        }
        self.enforce_limits()?;
        Ok(())
    }

    pub(crate) fn flush_pending_symbol(&self, symbol: &str) -> Result<()> {
        let entries = {
            let mut pending = self
                .pending
                .lock()
                .map_err(|e| crate::error::SdkError::Init(format!("缓存待写入状态已中毒: {e}")))?;
            let keys: Vec<_> = pending
                .keys()
                .filter(|key| key.symbol == symbol)
                .cloned()
                .collect();
            keys.into_iter()
                .filter_map(|key| pending.remove(&key).map(|bar| (key, bar)))
                .collect::<Vec<_>>()
        };
        self.flush_pending_entries(entries)
    }

    pub(crate) fn flush_pending_all(&self) -> Result<()> {
        let entries = {
            let mut pending = self
                .pending
                .lock()
                .map_err(|e| crate::error::SdkError::Init(format!("缓存待写入状态已中毒: {e}")))?;
            std::mem::take(&mut *pending)
                .into_iter()
                .collect::<Vec<_>>()
        };
        self.flush_pending_entries(entries)
    }

    fn apply_large_duration_quote(
        &self,
        quote: &Quote,
        duration_ns: i64,
        bucket_id: i64,
        delta_volume: f64,
    ) -> Result<bool> {
        let key = PendingKey::new(&quote.symbol, duration_ns);
        let latest = self
            .series_cache
            .read_latest_kline_rows(&quote.symbol, duration_ns, 1)?;

        let mut pending = self
            .pending
            .lock()
            .map_err(|e| crate::error::SdkError::Init(format!("缓存待写入状态已中毒: {e}")))?;

        if let Some(mut bar) = pending.remove(&key) {
            if bucket_id < bar.id {
                pending.insert(key, bar);
                return Ok(false);
            }

            if bucket_id == bar.id {
                Self::update_bar(&mut bar, quote, delta_volume);
                pending.insert(key, bar);
                return Ok(false);
            }

            let next_bar = Self::build_bar(duration_ns, bucket_id, quote, delta_volume);
            drop(pending);
            self.series_cache
                .upsert_last_kline_row(&quote.symbol, duration_ns, &bar)?;
            let mut pending = self
                .pending
                .lock()
                .map_err(|e| crate::error::SdkError::Init(format!("缓存待写入状态已中毒: {e}")))?;
            pending.insert(key, next_bar);
            return Ok(true);
        }

        if let Some(last) = latest.last() {
            if bucket_id < last.id {
                return Ok(false);
            }

            if bucket_id == last.id {
                let mut bar = last.clone();
                Self::update_bar(&mut bar, quote, delta_volume);
                pending.insert(key, bar);
                return Ok(false);
            }
        }

        pending.insert(
            key,
            Self::build_bar(duration_ns, bucket_id, quote, delta_volume),
        );
        Ok(false)
    }

    /// 获取指定范围的数据
    pub fn get(&self, key: &CacheKey, start_id: i64, end_id: i64) -> Vec<KlineBar> {
        if end_id <= start_id {
            return Vec::new();
        }

        let duration_ns = Self::series_duration_ns(key);
        if duration_ns <= 0 {
            return Vec::new();
        }

        let rows = self
            .series_cache
            .read_kline_window_by_id(&key.symbol, duration_ns, start_id, end_id)
            .unwrap_or_default();
        self.overlay_pending(key, rows)
    }

    /// 获取最新的若干条数据
    pub fn latest(&self, key: &CacheKey, count: usize) -> Vec<KlineBar> {
        if count == 0 {
            return Vec::new();
        }

        let duration_ns = Self::series_duration_ns(key);
        if duration_ns <= 0 {
            return Vec::new();
        }

        let rows = self
            .series_cache
            .read_latest_kline_rows(&key.symbol, duration_ns, count)
            .unwrap_or_default();
        Self::tail_rows(self.overlay_pending(key, rows), count)
    }

    /// 存储数据
    pub fn put(&self, key: &CacheKey, bars: &[KlineBar]) -> Result<()> {
        if bars.is_empty() {
            return Ok(());
        }

        let duration_ns = Self::series_duration_ns(key);
        if duration_ns <= 0 {
            return Ok(());
        }

        let mut merged = BTreeMap::new();
        for bar in self.all_rows(key)? {
            merged.insert(bar.id, bar);
        }
        for bar in bars {
            merged.insert(bar.id, bar.clone());
        }

        let merged_bars: Vec<KlineBar> = merged.into_values().collect();
        self.series_cache
            .replace_kline_rows(&key.symbol, duration_ns, &merged_bars)?;
        self.enforce_limits()?;

        tracing::debug!(
            "cached {} bars for {} (id {}-{})",
            bars.len(),
            key.symbol,
            bars.first().map(|b| b.id).unwrap_or(0),
            bars.last().map(|b| b.id).unwrap_or(0)
        );

        Ok(())
    }

    /// 用最新行情更新所有已缓存周期的尾部 K 线。
    pub fn apply_quote(&self, previous: Option<&Quote>, quote: &Quote) -> Result<()> {
        let durations = self.series_cache.list_durations(&quote.symbol)?;
        let mut grew_on_disk = false;
        for duration_ns in durations {
            if duration_ns <= 0 {
                continue;
            }

            let duration_secs = duration_ns / 1_000_000_000;
            if duration_secs <= 0 {
                continue;
            }

            let bucket_id = quote.timestamp.div_euclid(duration_secs);
            let delta_volume = match previous {
                Some(prev) if prev.symbol == quote.symbol && quote.volume >= prev.volume => {
                    quote.volume - prev.volume
                }
                _ => 0.0,
            };
            grew_on_disk |=
                self.apply_large_duration_quote(quote, duration_ns, bucket_id, delta_volume)?;
        }

        if grew_on_disk {
            self.enforce_limits()?;
        }

        Ok(())
    }

    /// 获取缺失的范围
    pub fn missing_ranges(&self, key: &CacheKey, start_id: i64, end_id: i64) -> Vec<Range> {
        let have = self.cached_ranges(key);
        let need = RangeSet::from_ranges(vec![(start_id, end_id)]);
        crate::storage::rangeset::rangeset_difference(&have, &need)
    }

    /// 获取已缓存的范围
    pub fn cached_ranges(&self, key: &CacheKey) -> RangeSet {
        let duration_ns = Self::series_duration_ns(key);
        if duration_ns <= 0 {
            return RangeSet::new();
        }

        let mut ranges = self
            .series_cache
            .get_rangeset_id(&key.symbol, duration_ns)
            .map(RangeSet::from_ranges)
            .unwrap_or_default();
        if let Some(pending_bar) = self.pending_bar(key) {
            ranges.insert(pending_bar.id, pending_bar.id + 1);
        }
        ranges
    }

    /// 检查是否完全缓存
    pub fn is_complete(&self, key: &CacheKey, start_id: i64, end_id: i64) -> bool {
        self.missing_ranges(key, start_id, end_id).is_empty()
    }

    /// 获取缓存统计
    pub fn stats(&self) -> CacheStats {
        let entries = self
            .series_cache
            .list_entries()
            .map(|entries| {
                entries
                    .into_iter()
                    .filter_map(|entry| {
                        if entry.duration_ns <= 0 {
                            return None;
                        }
                        Some(CacheEntryMeta {
                            symbol: entry.symbol,
                            duration: Duration {
                                ns: entry.duration_ns as u64,
                            },
                            ranges: RangeSet::from_ranges(entry.ranges),
                            updated_at: entry.updated_at,
                            bar_count: entry.bar_count,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        CacheStats {
            total_entries: entries.len(),
            total_bars: entries.iter().map(|entry| entry.bar_count).sum(),
            entries,
        }
    }

    /// 清除指定缓存
    pub fn clear(&self, key: &CacheKey) -> Result<()> {
        let duration_ns = Self::series_duration_ns(key);
        if duration_ns <= 0 {
            return Ok(());
        }
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(&PendingKey::new(&key.symbol, duration_ns));
        }
        self.series_cache.clear_series(&key.symbol, duration_ns)
    }

    /// 清除所有缓存
    pub fn clear_all(&self) -> Result<()> {
        if let Ok(mut pending) = self.pending.lock() {
            pending.clear();
        }
        self.series_cache.clear_all()
    }
}

/// 缓存统计
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: usize,
    pub total_bars: usize,
    pub entries: Vec<CacheEntryMeta>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::types::Quote;
    use std::fs::{self, File};
    use std::io::Write;
    use std::path::Path;
    use tempfile::tempdir;

    fn make_bar(id: i64) -> KlineBar {
        KlineBar {
            id,
            datetime: id * 60_000_000_000,
            open: 100.0 + id as f64,
            high: 105.0 + id as f64,
            low: 95.0 + id as f64,
            close: 102.0 + id as f64,
            volume: 1000.0,
            open_interest: 5000.0,
        }
    }

    fn make_quote(symbol: &str, timestamp: i64, price: f64, volume: f64) -> Quote {
        Quote {
            symbol: symbol.to_string(),
            timestamp,
            price,
            volume,
            ..Default::default()
        }
    }

    fn write_legacy_cache_file(base_dir: &Path, key: &CacheKey, bars: &[KlineBar]) {
        let symbol_dir = base_dir.join(&key.symbol);
        fs::create_dir_all(&symbol_dir).unwrap();
        let mut file = File::create(symbol_dir.join(key.cache_filename())).unwrap();
        for bar in bars {
            file.write_all(&bar.id.to_le_bytes()).unwrap();
            file.write_all(&bar.datetime.to_le_bytes()).unwrap();
            file.write_all(&bar.open.to_le_bytes()).unwrap();
            file.write_all(&bar.high.to_le_bytes()).unwrap();
            file.write_all(&bar.low.to_le_bytes()).unwrap();
            file.write_all(&bar.close.to_le_bytes()).unwrap();
            file.write_all(&bar.volume.to_le_bytes()).unwrap();
            file.write_all(&bar.open_interest.to_le_bytes()).unwrap();
        }
        file.sync_all().unwrap();
    }

    #[tokio::test]
    async fn test_cache_put_get() {
        let dir = tempdir().unwrap();
        let cache = HistoryCache::open(dir.path()).unwrap();

        let key = CacheKey::new("hf_CL", Duration::minutes(5));
        let bars = vec![make_bar(1), make_bar(2), make_bar(3)];

        cache.put(&key, &bars).unwrap();

        let cache2 = HistoryCache::open(dir.path()).unwrap();
        let retrieved = cache2.get(&key, 0, 100);
        assert_eq!(retrieved.len(), 3);
        assert_eq!(retrieved[0].id, 1);
    }

    #[tokio::test]
    async fn test_cache_ignores_legacy_bin_files() {
        let dir = tempdir().unwrap();
        let key = CacheKey::new("hf_CL", Duration::minutes(5));
        write_legacy_cache_file(dir.path(), &key, &[make_bar(1), make_bar(2)]);

        let cache = HistoryCache::open(dir.path()).unwrap();
        let retrieved = cache.get(&key, 0, 10);

        assert!(retrieved.is_empty());
    }

    #[tokio::test]
    async fn test_missing_ranges() {
        let dir = tempdir().unwrap();
        let cache = HistoryCache::open(dir.path()).unwrap();

        let key = CacheKey::new("hf_CL", Duration::minutes(5));
        let bars = vec![make_bar(1), make_bar(2), make_bar(50), make_bar(51)];

        cache.put(&key, &bars).unwrap();

        let missing = cache.missing_ranges(&key, 1, 100);

        assert_eq!(missing.len(), 2);
    }

    #[tokio::test]
    async fn test_cache_put_deduplicates_by_id() {
        let dir = tempdir().unwrap();
        let cache = HistoryCache::open(dir.path()).unwrap();

        let key = CacheKey::new("hf_CL", Duration::minutes(5));
        cache
            .put(&key, &[make_bar(1), make_bar(2), make_bar(3)])
            .unwrap();
        cache
            .put(&key, &[make_bar(2), make_bar(3), make_bar(4)])
            .unwrap();

        let cache2 = HistoryCache::open(dir.path()).unwrap();
        let retrieved = cache2.get(&key, 0, 10);
        let ids: Vec<i64> = retrieved.iter().map(|b| b.id).collect();
        assert_eq!(ids, vec![1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn test_cache_latest_returns_tail_bars() {
        let dir = tempdir().unwrap();
        let cache = HistoryCache::open(dir.path()).unwrap();

        let key = CacheKey::new("hf_CL", Duration::minutes(5));
        cache
            .put(
                &key,
                &[
                    make_bar(1),
                    make_bar(2),
                    make_bar(3),
                    make_bar(4),
                    make_bar(5),
                ],
            )
            .unwrap();

        let latest = cache.latest(&key, 2);
        let ids: Vec<i64> = latest.iter().map(|b| b.id).collect();
        assert_eq!(ids, vec![4, 5]);
    }

    #[tokio::test]
    async fn test_cache_apply_quote_updates_latest_bar_for_same_bucket_after_flush() {
        let dir = tempdir().unwrap();
        let cache = HistoryCache::open(dir.path()).unwrap();
        let key = CacheKey::new("hf_CL", Duration::minutes(1));
        cache
            .put(
                &key,
                &[KlineBar {
                    id: 2,
                    datetime: 120 * 1_000_000_000,
                    open: 10.0,
                    high: 10.0,
                    low: 10.0,
                    close: 10.0,
                    volume: 0.0,
                    open_interest: 0.0,
                }],
            )
            .unwrap();

        let prev = make_quote("hf_CL", 121, 10.0, 100.0);
        let next = make_quote("hf_CL", 130, 12.0, 108.0);

        cache.apply_quote(Some(&prev), &next).unwrap();

        let in_process_latest = cache.latest(&key, 1);
        assert_eq!(in_process_latest.len(), 1);
        assert_eq!(in_process_latest[0].close, 12.0);

        let reopened = HistoryCache::open(dir.path()).unwrap();
        let disk_latest = reopened.latest(&key, 1);
        assert_eq!(disk_latest.len(), 1);
        assert_eq!(disk_latest[0].id, 2);
        assert_eq!(disk_latest[0].open, 10.0);
        assert_eq!(disk_latest[0].high, 10.0);
        assert_eq!(disk_latest[0].low, 10.0);
        assert_eq!(disk_latest[0].close, 10.0);
        assert_eq!(disk_latest[0].volume, 0.0);

        cache.flush_pending_symbol("hf_CL").unwrap();

        let reopened = HistoryCache::open(dir.path()).unwrap();
        let latest = reopened.latest(&key, 1);
        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0].id, 2);
        assert_eq!(latest[0].open, 10.0);
        assert_eq!(latest[0].high, 12.0);
        assert_eq!(latest[0].low, 10.0);
        assert_eq!(latest[0].close, 12.0);
        assert_eq!(latest[0].volume, 8.0);
    }

    #[tokio::test]
    async fn test_large_duration_quote_updates_stay_pending_until_flushed() {
        let dir = tempdir().unwrap();
        let cache = HistoryCache::open(dir.path()).unwrap();
        let key = CacheKey::new("hf_CL", Duration::hours(1));
        cache
            .put(
                &key,
                &[KlineBar {
                    id: 2,
                    datetime: 2 * 3600 * 1_000_000_000,
                    open: 10.0,
                    high: 10.0,
                    low: 10.0,
                    close: 10.0,
                    volume: 0.0,
                    open_interest: 0.0,
                }],
            )
            .unwrap();

        let prev = make_quote("hf_CL", 2 * 3600 + 1, 10.0, 100.0);
        let next = make_quote("hf_CL", 2 * 3600 + 30, 12.0, 108.0);

        cache.apply_quote(Some(&prev), &next).unwrap();

        let reopened = HistoryCache::open(dir.path()).unwrap();
        let disk_latest = reopened.latest(&key, 1);
        assert_eq!(disk_latest[0].close, 10.0);

        let in_process_latest = cache.latest(&key, 1);
        assert_eq!(in_process_latest[0].close, 12.0);

        cache.flush_pending_symbol("hf_CL").unwrap();

        let reopened = HistoryCache::open(dir.path()).unwrap();
        let flushed = reopened.latest(&key, 1);
        assert_eq!(flushed[0].close, 12.0);
        assert_eq!(flushed[0].high, 12.0);
        assert_eq!(flushed[0].volume, 8.0);
    }

    #[tokio::test]
    async fn test_cache_apply_quote_appends_new_bar_for_next_bucket() {
        let dir = tempdir().unwrap();
        let cache = HistoryCache::open(dir.path()).unwrap();
        let key = CacheKey::new("hf_CL", Duration::minutes(1));
        cache
            .put(
                &key,
                &[KlineBar {
                    id: 2,
                    datetime: 120 * 1_000_000_000,
                    open: 10.0,
                    high: 11.0,
                    low: 10.0,
                    close: 11.0,
                    volume: 5.0,
                    open_interest: 0.0,
                }],
            )
            .unwrap();

        let prev = make_quote("hf_CL", 179, 11.0, 105.0);
        let next = make_quote("hf_CL", 180, 13.0, 112.0);

        cache.apply_quote(Some(&prev), &next).unwrap();

        let in_process_latest = cache.latest(&key, 2);
        let in_process_ids: Vec<i64> = in_process_latest.iter().map(|b| b.id).collect();
        assert_eq!(in_process_ids, vec![2, 3]);
        assert_eq!(in_process_latest[1].open, 13.0);
        assert_eq!(in_process_latest[1].high, 13.0);
        assert_eq!(in_process_latest[1].low, 13.0);
        assert_eq!(in_process_latest[1].close, 13.0);
        assert_eq!(in_process_latest[1].volume, 7.0);

        let reopened = HistoryCache::open(dir.path()).unwrap();
        let disk_latest = reopened.latest(&key, 2);
        let disk_ids: Vec<i64> = disk_latest.iter().map(|b| b.id).collect();
        assert_eq!(disk_ids, vec![2]);

        cache.flush_pending_symbol("hf_CL").unwrap();

        let reopened = HistoryCache::open(dir.path()).unwrap();
        let latest = reopened.latest(&key, 2);
        let ids: Vec<i64> = latest.iter().map(|b| b.id).collect();
        assert_eq!(ids, vec![2, 3]);
        assert_eq!(latest[1].open, 13.0);
        assert_eq!(latest[1].high, 13.0);
        assert_eq!(latest[1].low, 13.0);
        assert_eq!(latest[1].close, 13.0);
        assert_eq!(latest[1].volume, 7.0);
    }
}
