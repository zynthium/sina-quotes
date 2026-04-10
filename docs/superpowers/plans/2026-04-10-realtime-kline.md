# Realtime Kline (历史维护 + 实时订阅) Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在不改变现有 `subscribe_quote() -> QuoteStream`（快照视图）使用方式的前提下，新增“逐笔 QuoteFeed + 实时 K 线订阅 + 本地历史缓存复用”，支持推送未完结 K 线与已完结 K 线事件。

**Architecture:** WebSocket 解析出的 `Quote` 同时写入 `QuoteStream`（watch 最新值）与 `QuoteFeed`（mpsc/broadcast 队列不丢中间 tick）。K 线引擎在订阅时先接入 `QuoteFeed` 并缓冲，然后拉取/读取历史初始化，回放缓冲 Quote 形成当前未完结 bar，之后持续从 `QuoteFeed` 聚合并广播 `KlineEvent`，闭合 bar 异步写入 `HistoryCache`。

**Tech Stack:** Rust, tokio (watch/broadcast/mpsc), reqwest, fastwebsockets, serde/serde_json, thiserror

---

## File Map (锁定边界)

**Modify**
- [stream.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/stream.rs): 增加 `QuoteFeedManager`（逐笔队列流）并保留现有 `QuoteStream/QuoteManager`。
- [ws_service.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/net/ws_service.rs): WebSocket 收到 Quote 后同时发布到 `QuoteManager` 与 `QuoteFeedManager`。
- [client.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/client.rs): `SinaQuotes` 增加 `quote_feed_manager`；新增 API `subscribe_realtime_kline()`；管理 `(symbol,duration)` 的 K 线引擎实例与订阅句柄。
- [history.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/net/history.rs): 历史 bar 的 `id` 改为“按时间桶稳定计算”，避免因每次拉取条数不同导致 id 变化，支撑缓存复用。
- [cache.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/storage/cache.rs): 修复二进制编码/解码对 `f64` 的处理；补充去重/覆盖策略，避免重复 append 导致缓存膨胀。
- [lib.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/lib.rs): 导出新增的 `RealtimeKline` / `KlineEvent`（若对外公开）。

**Create**
- `src/realtime_kline.rs`: `KlineEvent`、`RealtimeKline`、聚合器（bucket 计算、bar 更新/闭合逻辑）。

**Tests (仅内联)**
- 在上述改动文件内添加 `#[cfg(test)]`：覆盖 quote feed 不丢事件、bucket 切换、缓存编码/解码正确、稳定 id 计算。

---

## Task 1: 引入 QuoteFeed（逐笔队列流）并从 WebSocket 发布

**Files:**
- Modify: [stream.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/stream.rs)
- Modify: [ws_service.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/net/ws_service.rs)
- Modify: [client.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/client.rs)

- [ ] **Step 1: 在 stream.rs 写一个最小失败测试（广播队列能收到多条，watch 只能看到最后一条）**

在 [stream.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/stream.rs) 新增 `#[cfg(test)]`：

```rust
#[tokio::test]
async fn test_quote_feed_receives_all_updates() {
    let feed = QuoteFeedManager::new();
    let mut rx = feed.subscribe("hf_TEST").await;
    feed.publish(crate::data::types::Quote { symbol: "hf_TEST".into(), price: 1.0, ..Default::default() }).await;
    feed.publish(crate::data::types::Quote { symbol: "hf_TEST".into(), price: 2.0, ..Default::default() }).await;
    let q1 = rx.recv().await.unwrap();
    let q2 = rx.recv().await.unwrap();
    assert_eq!(q1.price, 1.0);
    assert_eq!(q2.price, 2.0);
}
```

- [ ] **Step 2: 运行测试，确认失败（类型不存在）**

Run:
```bash
cargo test --lib stream::tests::test_quote_feed_receives_all_updates
```
Expected: FAIL，提示 `QuoteFeedManager` 未定义。

- [ ] **Step 3: 实现 QuoteFeedManager**

在 [stream.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/stream.rs) 增加（参考现有 `QuoteManager` 风格）：
- `QuoteFeedManager { streams: Arc<RwLock<HashMap<String, broadcast::Sender<Quote>>>> }`
- `subscribe(symbol) -> broadcast::Receiver<Quote>`
- `publish(quote)`：若该 symbol 有 sender 则 `send`；若无则创建 sender（设置一个合理容量，比如 1024）并发送第一条

- [ ] **Step 4: 运行测试，确认通过**

Run:
```bash
cargo test --lib stream::tests::test_quote_feed_receives_all_updates
```
Expected: PASS

- [ ] **Step 5: 将 WsConnection 发布 Quote 到 feed**

调整 [ws_service.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/net/ws_service.rs)：
- `WsConnection` 新增字段 `quote_feed_manager: QuoteFeedManager`
- 在收到 `Ok(quote)` 时，同时调用：
  - `self.quote_manager.update(quote.clone()).await;`
  - `self.quote_feed_manager.publish(quote).await;`

并调整 [client.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/client.rs) 的 `start_websocket()` 构造 `WsConnection::new(...)` 传入 `quote_feed_manager.clone()`。

- [ ] **Step 6: 为 WsConnection 增加一个单元测试（发布路径可用）**

在 [ws_service.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/net/ws_service.rs) 添加一个不依赖真实网络的测试：直接调用 `quote_feed_manager.publish(...)` 并验证订阅能收到（覆盖“连接层持有 feed”这种 wiring）。

- [ ] **Step 7: 运行全量单测**

Run:
```bash
cargo test
```
Expected: PASS

---

## Task 2: 修复/增强历史缓存以支撑“下次不拉服务器”

**Why:** 当前缓存二进制解码把 `f64` 当 `i64` 读，会损坏数据；同时历史 bar `id` 需要稳定可重建，否则无法基于 id 范围复用缓存。

**Files:**
- Modify: [cache.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/storage/cache.rs)
- Modify: [history.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/net/history.rs)
- Modify: [client.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/client.rs)

- [ ] **Step 1: 为缓存编解码写失败测试（写入后读出应完全一致）**

在 [cache.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/storage/cache.rs) 的 tests 增加：

```rust
#[test]
fn test_cache_encode_decode_roundtrip() {
    let bars = vec![make_bar(1), make_bar(2)];
    let encoded = HistoryCache::encode_bars(&bars);
    let decoded = HistoryCache::decode_bars(&encoded, 0, 10);
    assert_eq!(decoded.len(), 2);
    assert!((decoded[0].open - bars[0].open).abs() < 1e-9);
    assert!((decoded[0].close - bars[0].close).abs() < 1e-9);
}
```

（需要把 `encode_bars/decode_bars` 暂时改为 `pub(crate)` 以便测试，或在 tests 模块内用 `super::HistoryCache::...` 访问。）

- [ ] **Step 2: 运行该测试，确认失败**

Run:
```bash
cargo test --lib storage::cache::tests::test_cache_encode_decode_roundtrip
```
Expected: FAIL（解码出的浮点值不一致）。

- [ ] **Step 3: 修复 decode_bars（按 f64 读取）**

在 [cache.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/storage/cache.rs) 的 `decode_bars`：
- `id/datetime` 用 `i64::from_le_bytes`
- `open/high/low/close/volume/open_interest` 用 `f64::from_le_bytes`

同时修正 `BAR_SIZE` 的说明（保持 64 字节即可）。

- [ ] **Step 4: 运行 roundtrip 测试确认通过**

Run:
```bash
cargo test --lib storage::cache::tests::test_cache_encode_decode_roundtrip
```
Expected: PASS

- [ ] **Step 5: 让历史 bar id 稳定（按时间桶计算）**

在 [history.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/net/history.rs)：
- 在拿到 `datetime` 后，使用 `period`（分钟）计算 duration_ns
- `id = datetime / duration_ns`（向下取整，i64）
- 并把 `datetime` 对齐到 bucket 起点：`datetime = id * duration_ns`

补一个单测：给定两个相邻分钟的 datetime，计算出的 id 应相差 1。

- [ ] **Step 6: 调整 client 的缓存读取策略（按“当前时间桶 - count”读取）**

在 [client.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/client.rs) 的 `_fetch_history_internal`：
- 计算 `now_bucket_id = (Utc::now().timestamp_nanos_opt()/duration_ns)`（用 chrono/Utc 已依赖）
- 计算 `start_id = now_bucket_id - (count as i64) + 1`，`end_id = now_bucket_id + 1`
- cache.get(key, start_id, end_id)，若命中数量足够则返回
- 若不够，再走网络拉取；拉取后 `cache.put` 写入（建议在写入前做一次按 id 去重/覆盖，避免重复 append）

- [ ] **Step 7: 运行全量单测**

Run:
```bash
cargo test
```
Expected: PASS

---

## Task 3: 实现 RealtimeKline（订阅、未完结推送、闭合写缓存）

**Files:**
- Create: `src/realtime_kline.rs`
- Modify: [client.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/client.rs)
- Modify: [lib.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/lib.rs)

- [ ] **Step 1: 先写单元测试定义 Kline 聚合行为（bucket 内更新、跨 bucket 闭合）**

在 `src/realtime_kline.rs` 内联 tests：

```rust
#[test]
fn test_bucket_rollover() {
    let dur = crate::data::types::Duration::minutes(1);
    let mut agg = KlineAggregator::new("hf_TEST".into(), dur);
    let q1 = crate::data::types::Quote { symbol: "hf_TEST".into(), price: 10.0, volume: 100.0, timestamp: 60, ..Default::default() };
    let q2 = crate::data::types::Quote { symbol: "hf_TEST".into(), price: 12.0, volume: 105.0, timestamp: 61, ..Default::default() };
    let q3 = crate::data::types::Quote { symbol: "hf_TEST".into(), price: 11.0, volume: 110.0, timestamp: 120, ..Default::default() };
    let e1 = agg.on_quote(q1).unwrap();
    assert!(!e1.is_completed);
    let e2 = agg.on_quote(q2).unwrap();
    assert!(!e2.is_completed);
    let e3 = agg.on_quote(q3).unwrap();
    assert!(e3.is_completed);
}
```

说明：测试里用 `timestamp` 作为“秒级业务时间”，实现时需要统一为 ns 或提供适配函数。

- [ ] **Step 2: 实现 KlineEvent / RealtimeKline / KlineAggregator**

`src/realtime_kline.rs`：
- `KlineEvent { bar: KlineBar, is_completed: bool }`
- `RealtimeKline { series: KlineSeries, rx: broadcast::Receiver<KlineEvent> }`
- `RealtimeKline::series()` 返回 clone
- `RealtimeKline::next()` 从 rx.recv 读下一条（处理 lagged/closed）
- `KlineAggregator`：
  - 维护 `current_bar: Option<KlineBar>`
  - 维护 `last_total_volume: Option<f64>` 用于累计成交量 delta
  - `bucket_id(ts) -> i64` 与 `bucket_start_ns(bucket_id) -> i64`
  - bucket 内：更新 high/low/close/volume(delta 累加)
  - 跨 bucket：发出 `is_completed=true`（旧 bar），并开启新 bar（发 `is_completed=false`）

- [ ] **Step 3: 在 client 里实现 subscribe_realtime_kline**

在 [client.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/client.rs)：
- `SinaQuotes` 增加 `realtime_kline_engines: Arc<RwLock<HashMap<String, EngineHandle>>>`
- key 仍用 `format!("{}_{}", symbol, duration.as_secs())`
- 若已存在引擎：直接返回 `RealtimeKline { series: engine.series.clone(), rx: engine.tx.subscribe() }`
- 若不存在：创建
  - 先从 `quote_feed_manager.subscribe(symbol)` 得到 rx
  - 同时启动历史加载 future（cache 优先）
  - 用 `tokio::select!` 在“历史未完成”阶段持续接收 quote 并放入 `Vec<Quote>` 缓冲
  - 历史完成后：初始化 series（`KlineSeries::from_bars`），然后回放缓冲 quote 到 aggregator（只影响当前 bucket），必要时推送一次 `is_completed=false` 的事件
  - 进入常态循环：持续 recv quote，调用 aggregator，更新 series（`handle_update` / `push` / `update_last`），并广播事件；当 `is_completed=true` 时把闭合 bar 异步写缓存

- [ ] **Step 4: 添加一个 tokio 测试验证“先订阅后拉历史不会漏掉”**

思路：
- 使用一个假的 `QuoteFeedManager`（真实实现也可），先连续 publish 两条 quote
- 同时让历史 future 人为 sleep 一小段（模拟 HTTP）
- 订阅 realtime kline 后应收到至少一条未完结事件，且 series.current 的 close 为最新值

- [ ] **Step 5: 在 lib.rs 导出新类型**

在 [lib.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/src/lib.rs)：
- `mod realtime_kline;`
- `pub use realtime_kline::{RealtimeKline, KlineEvent};`

- [ ] **Step 6: 运行全量单测 + clippy + fmt**

Run:
```bash
cargo fmt
cargo clippy -- -D warnings
cargo test
```
Expected: 全部通过

---

## Task 4: Example & 兼容性验证（不改现有调用）

**Files:**
- Modify: [examples/new_api.rs](file:///Users/joeslee/Projects/GitHub/sina-quotes/examples/new_api.rs)

- [ ] **Step 1: 添加一个最小 demo（订阅 1m 实时 K 线并打印未完结与闭合事件）**

保持原有示例代码不删，只追加一个可选分支/段落演示 `subscribe_realtime_kline`。

- [ ] **Step 2: 本地运行 example**

Run:
```bash
cargo run --example new_api
```
Expected: 能打印未完结 bar 更新与跨分钟闭合提示（注意：具体输出取决于行情是否活跃）。

---

## Acceptance Checklist
- [ ] `subscribe_quote()` 的行为不变（仍是 `QuoteStream` 快照）。
- [ ] K 线聚合使用 `QuoteFeed`，不会因为 watch 覆盖而丢中间 tick。
- [ ] 初始化采用“先订阅后拉历史 + 缓冲回放”，不会漏掉历史拉取窗口内的新行情。
- [ ] 对订阅方会推送未完结 bar（`is_completed=false`）与闭合 bar（`is_completed=true`）。
- [ ] 历史缓存 roundtrip 正确，且 id 稳定可复用，二次启动命中缓存时不必拉服务器（缺失才拉）。

