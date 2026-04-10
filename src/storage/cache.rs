//! 历史数据缓存管理器
//! 
//! 使用文件系统缓存历史 K 线数据，支持内存映射以提高读取性能。

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::data::types::{Duration, KlineBar};
use crate::storage::rangeset::{RangeSet, Range};

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

/// 缓存索引文件
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
    
    fn key_to_string(key: &CacheKey) -> String {
        format!("{}_{}", key.symbol, key.duration.as_secs())
    }
}

/// 历史数据缓存
pub struct HistoryCache {
    /// 缓存目录
    cache_dir: PathBuf,
    /// 内存中的索引
    index: RwLock<CacheIndex>,
}

impl HistoryCache {
    /// 创建或打开缓存
    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let cache_dir = path.into();
        
        // 创建目录（如果不存在）
        fs::create_dir_all(&cache_dir)?;
        
        let index_path = cache_dir.join("index.json");
        let index = if index_path.exists() {
            let mut file = File::open(&index_path)?;
            let mut contents = String::new();
            file.read_to_string(&mut contents)?;
            serde_json::from_str(&contents).unwrap_or_default()
        } else {
            CacheIndex::new()
        };
        
        Ok(Self {
            cache_dir,
            index: RwLock::new(index),
        })
    }
    
    /// 获取缓存目录
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }
    
    /// 获取指定范围的数据
    pub fn get(&self, key: &CacheKey, start_id: i64, end_id: i64) -> Vec<KlineBar> {
        let index = self.index.read().unwrap();
        let key_str = CacheIndex::key_to_string(key);
        
        let meta = match index.entries.get(&key_str) {
            Some(m) => m,
            None => return Vec::new(),
        };
        
        // 检查是否有交集
        if !meta.ranges.intersects(start_id, end_id) {
            return Vec::new();
        }
        
        // 读取数据文件
        let file_path = self.cache_dir.join(meta.symbol.clone()).join(key.cache_filename());
        if !file_path.exists() {
            return Vec::new();
        }
        
        match File::open(&file_path) {
            Ok(mut file) => {
                let mut data = Vec::new();
                if file.read_to_end(&mut data).is_ok() {
                    Self::decode_bars(&data, start_id, end_id)
                } else {
                    Vec::new()
                }
            }
            Err(_) => Vec::new(),
        }
    }
    
    /// 存储数据
    pub fn put(&self, key: &CacheKey, bars: &[KlineBar]) -> Result<()> {
        if bars.is_empty() {
            return Ok(());
        }
        
        // 按 ID 排序
        let mut sorted = bars.to_vec();
        sorted.sort_by_key(|b| b.id);
        
        // 写入数据文件
        let _meta = {
            let symbol_dir = self.cache_dir.join(&key.symbol);
            fs::create_dir_all(&symbol_dir)?;
            
            let file_path = symbol_dir.join(key.cache_filename());
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file_path)?;
            
            let encoded = Self::encode_bars(&sorted);
            file.write_all(&encoded)?;
            file.sync_all()?;
            
            // 更新索引
            let mut index = self.index.write().unwrap();
            let key_str = CacheIndex::key_to_string(key);
            
            let entry = index.entries.entry(key_str.clone()).or_insert_with(|| {
                CacheEntryMeta {
                    symbol: key.symbol.clone(),
                    duration: key.duration,
                    ranges: RangeSet::new(),
                    updated_at: now_secs(),
                    bar_count: 0,
                }
            });
            
            // 更新范围
            let min_id = sorted.first().map(|b| b.id).unwrap_or(0);
            let max_id = sorted.last().map(|b| b.id).unwrap_or(0) + 1;
            entry.ranges.insert(min_id, max_id);
            entry.bar_count += sorted.len();
            entry.updated_at = now_secs();
            
            entry.clone()
        };
        
        // 持久化索引
        self.save_index()?;
        
        tracing::debug!(
            "cached {} bars for {} (id {}-{})",
            bars.len(),
            key.symbol,
            bars.first().map(|b| b.id).unwrap_or(0),
            bars.last().map(|b| b.id).unwrap_or(0)
        );
        
        Ok(())
    }
    
    /// 获取缺失的范围
    pub fn missing_ranges(&self, key: &CacheKey, start_id: i64, end_id: i64) -> Vec<Range> {
        let index = self.index.read().unwrap();
        let key_str = CacheIndex::key_to_string(key);
        
        let have = index.entries.get(&key_str)
            .map(|e| e.ranges.clone())
            .unwrap_or_default();
        
        let need = RangeSet::from_ranges(vec![(start_id, end_id)]);
        
        crate::storage::rangeset::rangeset_difference(&have, &need)
    }
    
    /// 获取已缓存的范围
    pub fn cached_ranges(&self, key: &CacheKey) -> RangeSet {
        let index = self.index.read().unwrap();
        let key_str = CacheIndex::key_to_string(key);
        
        index.entries.get(&key_str)
            .map(|e| e.ranges.clone())
            .unwrap_or_default()
    }
    
    /// 检查是否完全缓存
    pub fn is_complete(&self, key: &CacheKey, start_id: i64, end_id: i64) -> bool {
        self.missing_ranges(key, start_id, end_id).is_empty()
    }
    
    /// 获取缓存统计
    pub fn stats(&self) -> CacheStats {
        let index = self.index.read().unwrap();
        CacheStats {
            total_entries: index.entries.len(),
            total_bars: index.entries.values().map(|e| e.bar_count).sum(),
            entries: index.entries.values().cloned().collect(),
        }
    }
    
    /// 清除指定缓存
    pub fn clear(&self, key: &CacheKey) -> Result<()> {
        let mut index = self.index.write().unwrap();
        let key_str = CacheIndex::key_to_string(key);
        
        // 删除数据文件
        let file_path = self.cache_dir.join(&key.symbol).join(key.cache_filename());
        if file_path.exists() {
            fs::remove_file(&file_path)?;
        }
        
        // 更新索引
        index.entries.remove(&key_str);
        
        // 持久化索引
        drop(index);
        self.save_index()?;
        
        Ok(())
    }
    
    /// 清除所有缓存
    pub fn clear_all(&self) -> Result<()> {
        let mut index = self.index.write().unwrap();
        
        // 删除所有数据文件
        for entry in index.entries.values() {
            let file_path = self.cache_dir.join(&entry.symbol).join(
                CacheKey::new(&entry.symbol, entry.duration).cache_filename()
            );
            if file_path.exists() {
                let _ = fs::remove_file(&file_path);
            }
        }
        
        // 清空索引
        index.entries.clear();
        
        // 持久化索引
        drop(index);
        self.save_index()?;
        
        Ok(())
    }
    
    /// 保存索引到文件
    fn save_index(&self) -> Result<()> {
        let index = self.index.read().unwrap();
        let index_path = self.cache_dir.join("index.json");
        let json = serde_json::to_string_pretty(&*index)?;
        let mut file = File::create(index_path)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()?;
        Ok(())
    }
    
    /// 编码 K线数据为二进制
    fn encode_bars(bars: &[KlineBar]) -> Vec<u8> {
        let mut data = Vec::with_capacity(bars.len() * 64);  // 估算大小
        
        for bar in bars {
            // 固定大小的二进制格式
            data.extend_from_slice(&bar.id.to_le_bytes());
            data.extend_from_slice(&bar.datetime.to_le_bytes());
            data.extend_from_slice(&bar.open.to_le_bytes());
            data.extend_from_slice(&bar.high.to_le_bytes());
            data.extend_from_slice(&bar.low.to_le_bytes());
            data.extend_from_slice(&bar.close.to_le_bytes());
            data.extend_from_slice(&bar.volume.to_le_bytes());
            data.extend_from_slice(&bar.open_interest.to_le_bytes());
        }
        
        data
    }
    
    /// 解码二进制数据为 K线
    fn decode_bars(data: &[u8], start_id: i64, end_id: i64) -> Vec<KlineBar> {
        const BAR_SIZE: usize = 8 * 8;  // 8个 i64 字段
        
        if data.len() < BAR_SIZE {
            return Vec::new();
        }
        
        data.chunks(BAR_SIZE)
            .filter_map(|chunk| {
                if chunk.len() < BAR_SIZE {
                    return None;
                }
                
                let mut offset = 0;
                let read_i64 = |chunk: &[u8], off: &mut usize| -> i64 {
                    let val = i64::from_le_bytes([
                        chunk[*off], chunk[*off+1], chunk[*off+2], chunk[*off+3],
                        chunk[*off+4], chunk[*off+5], chunk[*off+6], chunk[*off+7],
                    ]);
                    *off += 8;
                    val
                };
                
                let id = read_i64(chunk, &mut offset);
                let datetime = read_i64(chunk, &mut offset);
                let open = read_i64(chunk, &mut offset) as f64;
                let high = read_i64(chunk, &mut offset) as f64;
                let low = read_i64(chunk, &mut offset) as f64;
                let close = read_i64(chunk, &mut offset) as f64;
                let volume = read_i64(chunk, &mut offset) as f64;
                let open_interest = read_i64(chunk, &mut offset) as f64;
                
                if id >= start_id && id < end_id {
                    Some(KlineBar {
                        id,
                        datetime,
                        open,
                        high,
                        low,
                        close,
                        volume,
                        open_interest,
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

/// 缓存统计
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_entries: usize,
    pub total_bars: usize,
    pub entries: Vec<CacheEntryMeta>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_bar(id: i64) -> KlineBar {
        KlineBar {
            id,
            datetime: id * 60000000000,
            open: 100.0 + id as f64,
            high: 105.0 + id as f64,
            low: 95.0 + id as f64,
            close: 102.0 + id as f64,
            volume: 1000.0,
            open_interest: 5000.0,
        }
    }

    #[tokio::test]
    async fn test_cache_put_get() {
        let dir = tempdir().unwrap();
        let cache = HistoryCache::open(dir.path()).unwrap();
        
        let key = CacheKey::new("hf_CL", Duration::minutes(5));
        let bars = vec![make_bar(1), make_bar(2), make_bar(3)];
        
        cache.put(&key, &bars).unwrap();
        
        // 重新打开缓存
        let cache2 = HistoryCache::open(dir.path()).unwrap();
        
        let retrieved = cache2.get(&key, 0, 100);
        assert_eq!(retrieved.len(), 3);
        assert_eq!(retrieved[0].id, 1);
    }

    #[tokio::test]
    async fn test_missing_ranges() {
        let dir = tempdir().unwrap();
        let cache = HistoryCache::open(dir.path()).unwrap();
        
        let key = CacheKey::new("hf_CL", Duration::minutes(5));
        let bars = vec![make_bar(1), make_bar(2), make_bar(50), make_bar(51)];
        
        cache.put(&key, &bars).unwrap();
        
        // 应该有 3-50 和 52-100 的缺失
        let missing = cache.missing_ranges(&key, 0, 100);
        
        assert_eq!(missing.len(), 2);
    }
}
