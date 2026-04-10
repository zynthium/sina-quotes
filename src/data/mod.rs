//! 数据层 — 核心数据结构
//!
//! - [`types`] — K线、行情、时间周期等基础类型
//! - [`buffer`] — 环形缓冲区
//! - [`series`] — K线序列（类似 TqSdk get_kline_serial）

pub mod buffer;
pub mod series;
pub mod types;
