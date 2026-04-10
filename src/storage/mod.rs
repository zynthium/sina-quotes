//! 存储层 — 缓存与数据结构
//!
//! - [`cache`] — 历史数据文件缓存
//! - [`rangeset`] — 范围集合（用于缓存缺失检测）

pub mod cache;
pub mod rangeset;
