//! 最新行情快照缓存。

use fs2::FileExt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::data::types::Quote;
use crate::error::{Result, SdkError};

const DATE_BYTES: usize = 16;
const QUOTE_TIME_BYTES: usize = 16;
const NAME_BYTES: usize = 64;
const QUOTE_RECORD_SIZE: u64 =
    8 + (9 * 8) + DATE_BYTES as u64 + QUOTE_TIME_BYTES as u64 + NAME_BYTES as u64;

pub(crate) struct QuoteCache {
    base_path: PathBuf,
    in_process_lock: Mutex<()>,
}

struct QuoteCacheLock<'a> {
    _guard: std::sync::MutexGuard<'a, ()>,
    lock_file: File,
}

impl Drop for QuoteCacheLock<'_> {
    fn drop(&mut self) {
        let _ = self.lock_file.unlock();
    }
}

impl QuoteCache {
    pub(crate) fn new(base_path: impl Into<PathBuf>) -> Result<Self> {
        let base_path = base_path.into();
        fs::create_dir_all(&base_path)?;
        Ok(Self {
            base_path,
            in_process_lock: Mutex::new(()),
        })
    }

    pub(crate) fn put(&self, quote: &Quote) -> Result<()> {
        let _lock = self.lock_quote(&quote.symbol)?;
        fs::create_dir_all(&self.base_path)?;
        let path = self.quote_path(&quote.symbol);
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)?;
        file.set_len(QUOTE_RECORD_SIZE)?;
        file.seek(SeekFrom::Start(0))?;

        file.write_all(&quote.timestamp.to_ne_bytes())?;
        write_f64(&mut file, quote.price)?;
        write_f64(&mut file, quote.bid_price)?;
        write_f64(&mut file, quote.ask_price)?;
        write_f64(&mut file, quote.open)?;
        write_f64(&mut file, quote.high)?;
        write_f64(&mut file, quote.low)?;
        write_f64(&mut file, quote.volume)?;
        write_f64(&mut file, quote.prev_settle)?;
        write_f64(&mut file, quote.settle_price)?;
        file.write_all(&encode_fixed_str(&quote.date, DATE_BYTES))?;
        file.write_all(&encode_fixed_str(&quote.quote_time, QUOTE_TIME_BYTES))?;
        file.write_all(&encode_fixed_str(&quote.name, NAME_BYTES))?;
        file.sync_all()?;
        Ok(())
    }

    pub(crate) fn get(&self, symbol: &str) -> Result<Option<Quote>> {
        let path = self.quote_path(symbol);
        if !path.exists() {
            return Ok(None);
        }

        let mut file = File::open(path)?;
        if file.metadata()?.len() < QUOTE_RECORD_SIZE {
            return Ok(None);
        }

        let mut timestamp_bytes = [0u8; 8];
        file.read_exact(&mut timestamp_bytes)?;

        let mut date_bytes = [0u8; DATE_BYTES];
        let mut quote_time_bytes = [0u8; QUOTE_TIME_BYTES];
        let mut name_bytes = [0u8; NAME_BYTES];

        let quote = Quote {
            symbol: symbol.to_string(),
            timestamp: i64::from_ne_bytes(timestamp_bytes),
            price: read_f64(&mut file)?,
            bid_price: read_f64(&mut file)?,
            ask_price: read_f64(&mut file)?,
            open: read_f64(&mut file)?,
            high: read_f64(&mut file)?,
            low: read_f64(&mut file)?,
            volume: read_f64(&mut file)?,
            prev_settle: read_f64(&mut file)?,
            settle_price: read_f64(&mut file)?,
            date: {
                file.read_exact(&mut date_bytes)?;
                decode_fixed_str(&date_bytes)
            },
            quote_time: {
                file.read_exact(&mut quote_time_bytes)?;
                decode_fixed_str(&quote_time_bytes)
            },
            name: {
                file.read_exact(&mut name_bytes)?;
                decode_fixed_str(&name_bytes)
            },
        };

        Ok(Some(quote))
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

    fn lock_quote(&self, symbol: &str) -> Result<QuoteCacheLock<'_>> {
        let guard = self
            .in_process_lock
            .lock()
            .map_err(|e| SdkError::Init(format!("行情缓存写锁已中毒: {e}")))?;
        fs::create_dir_all(&self.base_path)?;
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(self.lock_path(symbol))?;
        lock_file
            .lock_exclusive()
            .map_err(|e| SdkError::Init(format!("获取行情缓存文件锁失败: {e}")))?;
        Ok(QuoteCacheLock {
            _guard: guard,
            lock_file,
        })
    }

    fn quote_path(&self, symbol: &str) -> PathBuf {
        self.base_path.join(format!("{symbol}.quote"))
    }

    fn lock_path(&self, symbol: &str) -> PathBuf {
        self.base_path.join(format!(".{symbol}.lock"))
    }
}

fn write_f64<W: Write>(writer: &mut W, value: f64) -> Result<()> {
    writer.write_all(&value.to_ne_bytes())?;
    Ok(())
}

fn read_f64<R: Read>(reader: &mut R) -> Result<f64> {
    let mut bytes = [0u8; 8];
    reader.read_exact(&mut bytes)?;
    Ok(f64::from_ne_bytes(bytes))
}

fn encode_fixed_str(value: &str, size: usize) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(size);
    for ch in value.chars() {
        let mut buf = [0u8; 4];
        let encoded = ch.encode_utf8(&mut buf).as_bytes();
        if bytes.len() + encoded.len() > size {
            break;
        }
        bytes.extend_from_slice(encoded);
    }
    bytes.resize(size, 0);
    bytes
}

fn decode_fixed_str(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn quote_roundtrip() {
        let dir = tempdir().unwrap();
        let cache = QuoteCache::new(dir.path()).unwrap();
        let quote = Quote {
            symbol: "hf_CL".to_string(),
            price: 80.5,
            bid_price: 80.4,
            ask_price: 80.6,
            volume: 123.0,
            prev_settle: 79.0,
            settle_price: 80.0,
            timestamp: 1_700_000_000,
            date: "2026-04-18".to_string(),
            quote_time: "15:00:00".to_string(),
            name: "原油".to_string(),
            ..Default::default()
        };

        cache.put(&quote).unwrap();
        let restored = cache.get("hf_CL").unwrap().unwrap();
        assert_eq!(restored.symbol, "hf_CL");
        assert_eq!(restored.price, 80.5);
        assert_eq!(restored.name, "原油");
        assert_eq!(restored.quote_time, "15:00:00");
    }
}
