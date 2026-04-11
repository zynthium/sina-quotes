---
name: sina-quotes
description: Use when the user wants to use the `sina-quotes` Rust library or CLI to fetch Sina Finance international futures / overseas commodity market data, especially `hf_` symbols like `hf_OIL`, `hf_CL`, `hf_GC`, `hf_SI`, including historical K-lines, real-time quotes, or real-time K-line aggregation. 也适用于“新浪 外盘 行情 / 外盘期货 / 外盘商品 / 国际期货 / hf_符号 / 历史K线 / 实时行情 / WebSocket / Rust SDK / CLI”等任务；不适用于 A 股、基金、财报或基本面数据。
---

# Sina Quotes

`sina-quotes` 是一个面向新浪财经外盘行情的 Rust 库，主要用于外盘期货、贵金属、能源、农产品等 `hf_` 合约的历史 K 线与实时行情获取。

## 适用范围

- 用户明确提到 `sina-quotes`、`hf_OIL`、`hf_CL`、`hf_GC`、`hf_SI` 等外盘符号
- 需要用 Rust 获取新浪财经外盘历史 K 线
- 需要订阅外盘实时 Quote
- 需要基于实时 Quote 聚合实时 K 线
- 需要用 CLI 或 `examples/` 快速验证外盘行情是否可读

## 不适用

- A 股、港股、美股普通股票场景
- 财报、基本面、公告、新闻、资讯抓取
- 通用证券分析库选型

## 先判断任务类型

1. 历史 K 线：读取 `references/library.md` 的“历史 K 线”模板
2. 实时 Quote：读取 `references/library.md` 的“实时 Quote”模板
3. 实时 K 线：读取 `references/library.md` 的“实时 K 线”模板
4. 仅想快速手动验证：读取 `references/cli.md`

## 必须遵守的约束

- 这是 `tokio` 异步库。默认给出异步 Rust 示例。
- 这个库主要处理外盘 `hf_` 符号。先说明它是“新浪财经外盘行情库”，再给代码。
- 历史 K 线入口是 `client.get_kline_serial(symbol, duration, count).await?`。
- `subscribe_quote()` / `subscribe_quotes()` 只创建本地流句柄；想收到持续更新，必须再调用 `start_websocket(vec![...]).await?`。
- `subscribe_realtime_kline()` 的实时事件同样依赖 `start_websocket(...)`。它基于 Quote 流聚合，不是第二套独立行情源。
- 同一个 `SinaQuotes` 客户端只能启动一个 WebSocket 连接。需要多个品种时，把所有符号一次性传给 `start_websocket(...)`。
- `Quote` 里的 `open`、`high`、`low`、`volume` 是当日累计字段，不是当前 K 线的 OHLCV。
- 在明确结束客户端生命周期时，优先调用 `client.close().await`。

## 默认输出方式

- 优先输出可直接复制运行的最小完整 Rust 示例。
- 如果用户只是想快速验证数据，优先给 CLI 命令或 `cargo run --example ...`。
- 做实时订阅时，必须把“创建订阅句柄”和“启动 WebSocket”两步都写出来，不能省略。
- 如果用户没给符号，优先举 `hf_OIL`、`hf_CL`、`hf_GC`、`hf_SI` 这类外盘例子。

## 参考资料

- Rust API、模板、常见坑：`references/library.md`
- CLI 与 `examples/` 用法：`references/cli.md`
