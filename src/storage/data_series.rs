//! tqsdk-rs 风格的 K 线段缓存。
//!
//! 采用固定字节记录 + 段文件命名：
//! `<symbol>.<duration_ns>.<start_id>.<end_id>`
//! 以支持高频尾部覆盖和追加写入。

use fs2::FileExt;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::data::types::KlineBar;
use crate::error::{Result, SdkError};
use crate::storage::rangeset::Range;

const BAR_RECORD_SIZE: u64 = 8 * 8;

#[derive(Debug, Clone)]
pub(crate) struct SeriesEntryMeta {
    pub(crate) symbol: String,
    pub(crate) duration_ns: i64,
    pub(crate) ranges: Vec<Range>,
    pub(crate) updated_at: u64,
    pub(crate) bar_count: usize,
}

pub(crate) struct DataSeriesCache {
    base_path: PathBuf,
    in_process_lock: Mutex<()>,
}

#[derive(Debug, Clone)]
struct CacheFileMeta {
    path: PathBuf,
    size_bytes: u64,
    modified: SystemTime,
}

struct DataSeriesCacheLock<'a> {
    _guard: std::sync::MutexGuard<'a, ()>,
    lock_file: File,
}

impl Drop for DataSeriesCacheLock<'_> {
    fn drop(&mut self) {
        let _ = self.lock_file.unlock();
    }
}

impl DataSeriesCache {
    pub(crate) fn new(base_path: impl Into<PathBuf>) -> Result<Self> {
        let base_path = base_path.into();
        fs::create_dir_all(&base_path)?;
        Ok(Self {
            base_path,
            in_process_lock: Mutex::new(()),
        })
    }

    pub(crate) fn get_rangeset_id(&self, symbol: &str, duration_ns: i64) -> Result<Vec<Range>> {
        let mut ranges = Vec::new();
        if !self.base_path.exists() {
            return Ok(ranges);
        }

        for entry in fs::read_dir(&self.base_path)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let filename = entry.file_name();
            let filename = filename.to_string_lossy();
            let Some((file_symbol, file_duration_ns, range)) = parse_data_file_name(&filename)
            else {
                continue;
            };
            if file_symbol == symbol && file_duration_ns == duration_ns {
                ranges.push(range);
            }
        }

        ranges.sort_by_key(|range| range.0);
        Ok(ranges)
    }

    pub(crate) fn list_durations(&self, symbol: &str) -> Result<Vec<i64>> {
        let mut durations = BTreeSet::new();
        if !self.base_path.exists() {
            return Ok(Vec::new());
        }

        for entry in fs::read_dir(&self.base_path)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let filename = entry.file_name();
            let filename = filename.to_string_lossy();
            let Some((file_symbol, duration_ns, _)) = parse_data_file_name(&filename) else {
                continue;
            };
            if file_symbol == symbol {
                durations.insert(duration_ns);
            }
        }

        Ok(durations.into_iter().collect())
    }

    pub(crate) fn replace_kline_rows(
        &self,
        symbol: &str,
        duration_ns: i64,
        rows: &[KlineBar],
    ) -> Result<()> {
        let _lock = self.lock_series(symbol, duration_ns)?;
        self.clear_series_locked(symbol, duration_ns)?;
        for segment in split_contiguous_rows(rows) {
            self.write_segment_locked(symbol, duration_ns, &segment)?;
        }
        Ok(())
    }

    pub(crate) fn upsert_last_kline_row(
        &self,
        symbol: &str,
        duration_ns: i64,
        row: &KlineBar,
    ) -> Result<()> {
        let _lock = self.lock_series(symbol, duration_ns)?;
        let ranges = self.get_rangeset_id(symbol, duration_ns)?;

        let Some((start_id, end_id)) = ranges.last().copied() else {
            self.write_segment_locked(symbol, duration_ns, std::slice::from_ref(row))?;
            return Ok(());
        };

        if row.id < end_id - 1 {
            return Ok(());
        }

        let current_path = self.get_data_file_path(symbol, duration_ns, (start_id, end_id));

        if row.id == end_id - 1 {
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&current_path)?;
            let row_index = (row.id - start_id) as u64;
            let offset = row_index
                .checked_mul(BAR_RECORD_SIZE)
                .ok_or_else(|| SdkError::Init("缓存偏移量溢出".to_string()))?;
            file.seek(SeekFrom::Start(offset))?;
            write_bar(&mut file, row)?;
            file.sync_all()?;
            return Ok(());
        }

        if row.id == end_id {
            let mut file = OpenOptions::new().append(true).open(&current_path)?;
            write_bar(&mut file, row)?;
            file.sync_all()?;
            drop(file);

            let new_path = self.get_data_file_path(symbol, duration_ns, (start_id, end_id + 1));
            fs::rename(current_path, new_path)?;
            return Ok(());
        }

        self.write_segment_locked(symbol, duration_ns, std::slice::from_ref(row))?;
        Ok(())
    }

    pub(crate) fn read_kline_window_by_id(
        &self,
        symbol: &str,
        duration_ns: i64,
        start_id: i64,
        end_id: i64,
    ) -> Result<Vec<KlineBar>> {
        if end_id <= start_id {
            return Ok(Vec::new());
        }

        let ranges = self.get_rangeset_id(symbol, duration_ns)?;
        let mut rows = BTreeMap::new();

        for range @ (range_start, range_end) in ranges {
            if range_end <= start_id || range_start >= end_id {
                continue;
            }

            let file_path = self.get_data_file_path(symbol, duration_ns, range);
            let mut file = File::open(file_path)?;

            let read_start_id = start_id.max(range_start);
            let read_end_id = end_id.min(range_end);
            let start_index = (read_start_id - range_start) as usize;
            let end_index = (read_end_id - range_start) as usize;

            for index in start_index..end_index {
                let bar = read_bar_at(&mut file, index)?;
                rows.insert(bar.id, bar);
            }
        }

        Ok(rows.into_values().collect())
    }

    pub(crate) fn read_latest_kline_rows(
        &self,
        symbol: &str,
        duration_ns: i64,
        count: usize,
    ) -> Result<Vec<KlineBar>> {
        if count == 0 {
            return Ok(Vec::new());
        }

        let mut remaining = count;
        let mut rows = Vec::new();
        let ranges = self.get_rangeset_id(symbol, duration_ns)?;

        for range @ (range_start, range_end) in ranges.into_iter().rev() {
            if remaining == 0 {
                break;
            }

            let available = (range_end - range_start).max(0) as usize;
            if available == 0 {
                continue;
            }

            let take = remaining.min(available);
            let file_path = self.get_data_file_path(symbol, duration_ns, range);
            let mut file = File::open(file_path)?;
            let start_index = available - take;
            for index in start_index..available {
                rows.push(read_bar_at(&mut file, index)?);
            }
            remaining -= take;
        }

        rows.sort_by_key(|bar| bar.id);
        Ok(rows)
    }

    pub(crate) fn clear_series(&self, symbol: &str, duration_ns: i64) -> Result<()> {
        let _lock = self.lock_series(symbol, duration_ns)?;
        self.clear_series_locked(symbol, duration_ns)
    }

    pub(crate) fn clear_all(&self) -> Result<()> {
        if !self.base_path.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(&self.base_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let _ = fs::remove_file(path);
            }
        }
        Ok(())
    }

    pub(crate) fn list_entries(&self) -> Result<Vec<SeriesEntryMeta>> {
        let mut grouped: HashMap<(String, i64), SeriesEntryMeta> = HashMap::new();
        if !self.base_path.exists() {
            return Ok(Vec::new());
        }

        for entry in fs::read_dir(&self.base_path)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }

            let filename = entry.file_name();
            let filename = filename.to_string_lossy();
            let Some((symbol, duration_ns, range)) = parse_data_file_name(&filename) else {
                continue;
            };

            let metadata = entry.metadata()?;
            let updated_at = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let entry = grouped
                .entry((symbol.clone(), duration_ns))
                .or_insert_with(|| SeriesEntryMeta {
                    symbol: symbol.clone(),
                    duration_ns,
                    ranges: Vec::new(),
                    updated_at,
                    bar_count: 0,
                });

            entry.ranges.push(range);
            entry.bar_count += (range.1 - range.0).max(0) as usize;
            entry.updated_at = entry.updated_at.max(updated_at);
        }

        let mut entries: Vec<_> = grouped
            .into_values()
            .map(|mut entry| {
                entry.ranges.sort_by_key(|range| range.0);
                entry
            })
            .collect();
        entries.sort_by(|a, b| {
            a.symbol
                .cmp(&b.symbol)
                .then_with(|| a.duration_ns.cmp(&b.duration_ns))
        });
        Ok(entries)
    }

    pub(crate) fn enforce_limits(&self, max_bytes: Option<u64>) -> Result<()> {
        let Some(limit) = max_bytes else {
            return Ok(());
        };

        let _cleanup_lock = self
            .in_process_lock
            .lock()
            .map_err(|e| SdkError::Init(format!("缓存写锁已中毒: {e}")))?;

        let mut files = self.list_cache_files()?;
        let mut total: u64 = files.iter().map(|file| file.size_bytes).sum();
        if total <= limit {
            return Ok(());
        }

        files.sort_by_key(|file| file.modified);
        for file in files {
            if total <= limit {
                break;
            }
            if fs::remove_file(&file.path).is_ok() {
                total = total.saturating_sub(file.size_bytes);
            }
        }

        Ok(())
    }

    fn lock_series(&self, symbol: &str, duration_ns: i64) -> Result<DataSeriesCacheLock<'_>> {
        let guard = self
            .in_process_lock
            .lock()
            .map_err(|e| SdkError::Init(format!("缓存写锁已中毒: {e}")))?;
        fs::create_dir_all(&self.base_path)?;
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(self.get_lock_path(symbol, duration_ns))?;
        lock_file
            .lock_exclusive()
            .map_err(|e| SdkError::Init(format!("获取缓存文件锁失败: {e}")))?;
        Ok(DataSeriesCacheLock {
            _guard: guard,
            lock_file,
        })
    }

    fn clear_series_locked(&self, symbol: &str, duration_ns: i64) -> Result<()> {
        if !self.base_path.exists() {
            return Ok(());
        }

        for entry in fs::read_dir(&self.base_path)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }

            let filename = entry.file_name();
            let filename = filename.to_string_lossy();
            let Some((file_symbol, file_duration_ns, _)) = parse_data_file_name(&filename) else {
                continue;
            };
            if file_symbol == symbol && file_duration_ns == duration_ns {
                let _ = fs::remove_file(entry.path());
            }
        }

        Ok(())
    }

    fn write_segment_locked(
        &self,
        symbol: &str,
        duration_ns: i64,
        rows: &[KlineBar],
    ) -> Result<()> {
        if rows.is_empty() {
            return Ok(());
        }

        fs::create_dir_all(&self.base_path)?;
        let temp_path = self.get_temp_path(symbol, duration_ns);
        let mut file = File::create(&temp_path)?;
        {
            let mut writer = BufWriter::new(&mut file);
            for row in rows {
                write_bar(&mut writer, row)?;
            }
            writer.flush()?;
        }
        file.sync_all()?;

        let start_id = rows.first().map(|row| row.id).unwrap_or(0);
        let end_id = rows.last().map(|row| row.id + 1).unwrap_or(start_id);
        let target_path = self.get_data_file_path(symbol, duration_ns, (start_id, end_id));
        fs::rename(temp_path, target_path)?;
        Ok(())
    }

    fn get_data_file_path(&self, symbol: &str, duration_ns: i64, range: Range) -> PathBuf {
        self.base_path
            .join(format!("{symbol}.{duration_ns}.{}.{}", range.0, range.1))
    }

    fn get_temp_path(&self, symbol: &str, duration_ns: i64) -> PathBuf {
        self.base_path.join(format!("{symbol}.{duration_ns}.temp"))
    }

    fn get_lock_path(&self, symbol: &str, duration_ns: i64) -> PathBuf {
        self.base_path.join(format!(".{symbol}.{duration_ns}.lock"))
    }

    fn list_cache_files(&self) -> Result<Vec<CacheFileMeta>> {
        if !self.base_path.exists() {
            return Ok(Vec::new());
        }

        let mut files = Vec::new();
        for entry in fs::read_dir(&self.base_path)? {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }

            let filename = entry.file_name();
            let filename = filename.to_string_lossy();
            if parse_data_file_name(&filename).is_none() {
                continue;
            }

            let metadata = entry.metadata()?;
            files.push(CacheFileMeta {
                path: entry.path(),
                size_bytes: metadata.len(),
                modified: metadata.modified().unwrap_or(UNIX_EPOCH),
            });
        }

        Ok(files)
    }
}

fn split_contiguous_rows(rows: &[KlineBar]) -> Vec<Vec<KlineBar>> {
    let mut deduped = BTreeMap::new();
    for row in rows {
        deduped.insert(row.id, row.clone());
    }
    let rows: Vec<KlineBar> = deduped.into_values().collect();
    if rows.is_empty() {
        return Vec::new();
    }

    let mut segments = Vec::new();
    let mut current = vec![rows[0].clone()];

    for row in rows.iter().skip(1) {
        let prev = current.last().unwrap();
        if row.id == prev.id + 1 {
            current.push(row.clone());
        } else {
            segments.push(current);
            current = vec![row.clone()];
        }
    }
    segments.push(current);
    segments
}

fn parse_data_file_name(filename: &str) -> Option<(String, i64, Range)> {
    if filename.starts_with('.') || filename.ends_with(".lock") || filename.ends_with(".temp") {
        return None;
    }

    let parts: Vec<&str> = filename.split('.').collect();
    if parts.len() < 4 {
        return None;
    }

    let end = parts.last()?.parse::<i64>().ok()?;
    let start = parts.get(parts.len() - 2)?.parse::<i64>().ok()?;
    let duration_ns = parts.get(parts.len() - 3)?.parse::<i64>().ok()?;
    if start >= end {
        return None;
    }

    let symbol = parts[..parts.len() - 3].join(".");
    if symbol.is_empty() {
        return None;
    }

    Some((symbol, duration_ns, (start, end)))
}

fn read_bar_at(file: &mut File, index: usize) -> Result<KlineBar> {
    let offset = (index as u64)
        .checked_mul(BAR_RECORD_SIZE)
        .ok_or_else(|| SdkError::Init("缓存偏移量溢出".to_string()))?;
    file.seek(SeekFrom::Start(offset))?;
    read_bar(file)
}

fn write_bar<W: Write>(writer: &mut W, bar: &KlineBar) -> Result<()> {
    writer.write_all(&bar.id.to_ne_bytes())?;
    writer.write_all(&bar.datetime.to_ne_bytes())?;
    writer.write_all(&bar.open.to_ne_bytes())?;
    writer.write_all(&bar.high.to_ne_bytes())?;
    writer.write_all(&bar.low.to_ne_bytes())?;
    writer.write_all(&bar.close.to_ne_bytes())?;
    writer.write_all(&bar.volume.to_ne_bytes())?;
    writer.write_all(&bar.open_interest.to_ne_bytes())?;
    Ok(())
}

fn read_bar<R: Read>(reader: &mut R) -> Result<KlineBar> {
    Ok(KlineBar {
        id: read_i64(reader)?,
        datetime: read_i64(reader)?,
        open: read_f64(reader)?,
        high: read_f64(reader)?,
        low: read_f64(reader)?,
        close: read_f64(reader)?,
        volume: read_f64(reader)?,
        open_interest: read_f64(reader)?,
    })
}

fn read_i64<R: Read>(reader: &mut R) -> Result<i64> {
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(i64::from_ne_bytes(bytes))
}

fn read_f64<R: Read>(reader: &mut R) -> Result<f64> {
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(f64::from_ne_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_bar(id: i64, close: f64) -> KlineBar {
        KlineBar {
            id,
            datetime: id * 60 * 1_000_000_000,
            open: close - 1.0,
            high: close + 1.0,
            low: close - 2.0,
            close,
            volume: id as f64,
            open_interest: 0.0,
        }
    }

    #[test]
    fn read_latest_reads_tail_across_segments() {
        let dir = tempdir().unwrap();
        let cache = DataSeriesCache::new(dir.path()).unwrap();

        cache
            .replace_kline_rows(
                "hf_TEST",
                60 * 1_000_000_000,
                &[make_bar(1, 11.0), make_bar(2, 12.0), make_bar(5, 15.0)],
            )
            .unwrap();

        let latest = cache
            .read_latest_kline_rows("hf_TEST", 60 * 1_000_000_000, 2)
            .unwrap();
        let ids: Vec<i64> = latest.iter().map(|bar| bar.id).collect();
        assert_eq!(ids, vec![2, 5]);
    }

    #[test]
    fn upsert_last_row_overwrites_and_appends() {
        let dir = tempdir().unwrap();
        let cache = DataSeriesCache::new(dir.path()).unwrap();
        let duration_ns = 60 * 1_000_000_000;

        cache
            .replace_kline_rows(
                "hf_TEST",
                duration_ns,
                &[make_bar(1, 11.0), make_bar(2, 12.0)],
            )
            .unwrap();
        cache
            .upsert_last_kline_row("hf_TEST", duration_ns, &make_bar(2, 20.0))
            .unwrap();
        cache
            .upsert_last_kline_row("hf_TEST", duration_ns, &make_bar(3, 30.0))
            .unwrap();

        let latest = cache
            .read_latest_kline_rows("hf_TEST", duration_ns, 3)
            .unwrap();
        let closes: Vec<f64> = latest.iter().map(|bar| bar.close).collect();
        assert_eq!(closes, vec![11.0, 20.0, 30.0]);
    }

    #[test]
    fn enforce_limits_evicts_oldest_files_first() {
        let dir = tempdir().unwrap();
        let cache = DataSeriesCache::new(dir.path()).unwrap();
        let duration_ns = 60 * 1_000_000_000;

        cache
            .replace_kline_rows(
                "hf_OLD",
                duration_ns,
                &[make_bar(1, 11.0), make_bar(2, 12.0)],
            )
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        cache
            .replace_kline_rows(
                "hf_NEW",
                duration_ns,
                &[make_bar(1, 21.0), make_bar(2, 22.0)],
            )
            .unwrap();

        cache.enforce_limits(Some(BAR_RECORD_SIZE * 2)).unwrap();

        let entries = cache.list_entries().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].symbol, "hf_NEW");
    }
}
