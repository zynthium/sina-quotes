//! 存储层 — 缓存与数据结构
//!
//! - [`cache`] — 历史数据文件缓存
//! - [`data_series`] — 定长段缓存
//! - [`quote_cache`] — 最新行情快照缓存
//! - [`rangeset`] — 范围集合（用于缓存缺失检测）

pub mod cache;
pub mod data_series;
pub mod quote_cache;
pub mod rangeset;
