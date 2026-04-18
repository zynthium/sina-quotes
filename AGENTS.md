# AGENTS.md - SinaQuotes SDK

## 项目类型

Rust 库 + CLI 工具，用于获取新浪财经数据（历史 K 线和实时行情 WebSocket 订阅）。

## 开发命令

```bash
# 构建
cargo build

# 运行 CLI - 获取 K 线数据
cargo run -- klines hf_OIL 5

# 运行 CLI - 获取 K 线并指定数量
cargo run -- klines hf_GC 15 --count 200

# 运行 CLI - 订阅实时行情
cargo run -- subscribe hf_OIL hf_GC

# 代码检查
cargo clippy -- -D warnings

# 代码格式化
cargo fmt

# 测试（内联在 src/ 中）
cargo test
```

注意：`cargo test --lib` 只运行库单元测试。

## 架构

```
src/
├── lib.rs              # 公共导出 (pub use ...)
├── main.rs             # CLI 入口 (clap)
├── client.rs           # SinaQuotes 客户端（主要 API）
├── stream.rs           # QuoteStream（实时行情）
├── symbols.rs          # 外盘期货品种符号
├── error.rs            # SdkError
├── data/               # 数据层
│   ├── types.rs        # KlineBar, Quote, Duration
│   ├── buffer.rs       # 环形缓冲区
│   └── series.rs       # KlineSeries（K 线序列）
├── net/                # 网络层
│   ├── ws.rs           # WebSocket 协议
│   ├── ws_service.rs   # WebSocket 连接管理
│   └── history.rs      # 历史数据获取
└── storage/            # 存储层
    ├── cache.rs        # 历史数据缓存
    └── rangeset.rs     # RangeSet 数据结构
```

- 入口点：`SinaQuotes::new()` 或 `SinaQuotes::builder()...build()`
- 主要 API：`client.get_kline_serial()`、`client.subscribe_quote()`

## 关键约定

- **错误处理**：使用 `thiserror` 定义 `SdkError` 枚举
- **异步**：tokio 运行时，全面使用 async/await
- **测试**：仅使用内联 `#[cfg(test)]` 模块，无 `tests/` 目录
- **可见性**：在 lib.rs 中通过 `pub use` 导出公共 API
- **WebSocket**：使用 `fastwebsockets` crate + 自定义协议

## 值得注意的依赖

- `tokio` - 异步运行时
- `reqwest` - HTTP 客户端
- `fastwebsockets` - WebSocket
- `hyper` / `hyper-util` - HTTP/1.1
- `serde` / `serde_json` - 序列化
- `thiserror` - 错误类型
- `clap` - CLI

## 样式参考

遵循全局 Rust 规则：`~/.claude/rules/rust/`


## 版本管理

自动维护版本号，采用semver规范。