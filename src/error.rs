//! 统一错误类型

use thiserror::Error;

/// SDK 统一错误类型
#[derive(Error, Debug)]
#[error("SDK错误: {0}")]
pub enum SdkError {
    // 连接相关
    #[error("连接失败: {0}")]
    Connect(String),

    #[error("连接超时")]
    Timeout,

    #[error("网络错误: {0}")]
    Network(#[from] std::io::Error),

    #[error("HTTP请求失败: {0}")]
    Http(#[from] reqwest::Error),

    // WebSocket相关
    #[error("WebSocket错误: {0}")]
    WebSocket(String),

    #[error("WebSocket连接已关闭")]
    WsClosed,

    // 数据解析
    #[error("数据解析失败: {0}")]
    Parse(String),

    #[error("JSON解析失败: {0}")]
    Json(#[from] serde_json::Error),

    #[error("数据格式错误: 期望{expected}, 实际{actual}")]
    Format {
        expected: &'static str,
        actual: String,
    },

    // 业务逻辑
    #[error("历史数据不可用: {0}")]
    HistoryUnavailable(String),

    #[error("交易时间信息不可用: {category}/{symbol}")]
    MarketHoursUnavailable { category: String, symbol: String },

    #[error("符号不存在: {0}")]
    SymbolNotFound(String),

    #[error("K线ID间隙过大: 期望{expected}, 实际{actual}")]
    IdGap { expected: i64, actual: i64 },

    // SDK状态
    #[error("SDK已关闭")]
    Closed,

    #[error("SDK初始化失败: {0}")]
    Init(String),

    #[error("通道已关闭")]
    ChannelClosed,
}

pub type Result<T> = std::result::Result<T, SdkError>;

impl From<SdkError> for std::io::Error {
    fn from(e: SdkError) -> Self {
        std::io::Error::other(e.to_string())
    }
}
