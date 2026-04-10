//! 网络层 — HTTP 与 WebSocket 通信
//!
//! - [`history`] — 历史 K线数据获取
//! - [`ws`] — WebSocket 订阅
//! - [`ws_service`] — WebSocket 连接管理（自动重连）

pub mod history;
pub mod ws;
pub mod ws_service;
