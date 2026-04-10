# 实时 K 线订阅与聚合引擎设计

## 1. 目标与场景
支持从初始历史数据加载到实时 Quote 驱动的增量更新。对于任意级别 K 线（最小如 1m，更大如 5m、15m 等）：
- 初次加载时，尝试从本地缓存读取，若缺失则通过 HTTP 接口从服务器拉取历史。
- 随后通过 WebSocket 的 Quote 实时流继续填充当前 K 线。
- 对于正在形成（未完结）的 K 线，也需要实时推送给订阅方。
- 完整闭合的 K 线（如跨越 1m 边界）会被持久化到本地缓存，以便下次启动或其它策略复用，无需重复请求服务器。

## 2. 核心架构与数据流

### 2.1 架构分层
1. **底层数据源**: `QuoteFeed` (WebSocket 实时逐笔、尽量不丢事件的队列流) + `QuoteStream` (最新快照视图) + `HistoryFetcher` (HTTP 历史拉取)。
2. **K线引擎层 (KlineEngine)**: 
   - 作为一个独立的后台 Tokio 任务运行。
   - 负责管理特定 Symbol + Duration 的生命周期。
   - 维护一个内部 `KlineRingBuffer`（或现有的 `KlineSeries`）。
3. **订阅分发层**: 
   - 暴露 `RealtimeKline` 对象给用户，提供共享数据视图 `series()` 和事件流监听 `changed()` / `next()`。

### 2.2 聚合策略（为什么从 Quote 聚合更佳）
尽管 1m 可以作为最小级别 K 线，再由 1m 向上聚合 5m，但由于各级别 K 线的生成逻辑高度一致（仅仅是时间桶 `bucket` 的跨度不同），为了保证架构简单统一且无时延，**本设计建议所有级别的实时 K 线均直接由 Quote 驱动聚合**。
- `1m K线` = `Quote` 落入 60 秒的 bucket
- `5m K线` = `Quote` 落入 300 秒的 bucket
- **Trade-off**: 这样避免了级联订阅（Quote -> 1m -> 5m）可能带来的事件丢失和复杂状态机，且 K 线 OHLCV 合成计算极其轻量，不会成为性能瓶颈。

### 2.3 数据流向 (Data Flow)
1. **订阅触发**: 用户调用 `client.subscribe_realtime_kline("SHFE.au2602", Duration::minutes(1), 300)`。
2. **初始化**: 
   - SDK 检查是否已有该 `(symbol, duration)` 的 `KlineEngine` 实例。
   - 若无，则创建：
     - 先订阅底层 `QuoteFeed`（逐笔队列流），开始缓冲 Quote（避免历史拉取时间窗口内遗漏）。
     - 再通过 `HistoryCache` / `HTTP` 拉取最新 300 根历史 bar，初始化内部状态。
     - 拉取完成后，将缓冲区内的 Quote 按时间顺序重放到聚合器上，生成/更新“当前未完结 bar”，并向订阅方推送 `is_completed = false` 的更新事件。
3. **实时更新**:
   - 后台任务从 `QuoteFeed` 循环读取 Quote（注意：仅 `QuoteStream`/watch 快照无法保证不丢中间 tick，不适合作为 OHLCV 聚合的输入）。
   - 根据 `quote.timestamp` 或 `quote.quote_time` 计算当前所属的 K线时间桶 (Bucket)。
   - **更新未完结 bar**: 如果 Bucket 相同，更新当前 bar 的 High/Low/Close/Volume。通过广播通道推送 `KlineEvent { bar, is_completed: false }`。
   - **完结旧 bar**: 如果 Bucket 变化（时间跨入下一个 K 线周期），则前一个 bar 宣告完结，推送 `KlineEvent { bar: old_bar, is_completed: true }`，并将 `old_bar` 异步写入本地 `HistoryCache`。同时开启新 bar 并初始化为当前 quote 的状态。
4. **消费分发**:
   - `RealtimeKline` 返回给用户，用户不仅能随时 `series.current()` 读快照，还能 `while let Some(event) = sub.next().await` 获取未完结/已完结 K 线的精确事件。

## 3. 接口设计

### 3.1 用户使用示例

```rust
// 1. 获取实时 K 线订阅对象 (包含历史 + 实时增量)
let mut sub = client
    .subscribe_realtime_kline("SHFE.au2602", Duration::minutes(1), 300)
    .await?;

// 2. 获取共享状态句柄 (可以传递给 UI 或其他线程用于快照读取)
let series = sub.series();

// 3. 事件驱动消费
while let Some(event) = sub.next().await {
    if event.is_completed {
        println!("新 K 线闭合: id={} close={}", event.bar.id, event.bar.close);
    } else {
        println!("当前 K 线更新 (未完结): id={} close={}", event.bar.id, event.bar.close);
    }
}
```

### 3.2 核心结构体定义

```rust
/// K 线更新事件
#[derive(Debug, Clone)]
pub struct KlineEvent {
    pub bar: KlineBar,
    pub is_completed: bool, // true: 跨周期旧 bar 完结; false: 当前 bar 更新
}

/// 实时 K 线订阅句柄
pub struct RealtimeKline {
    series: KlineSeries,                     // 内部状态的共享视图
    rx: tokio::sync::broadcast::Receiver<KlineEvent>, // 事件接收端
}

impl RealtimeKline {
    /// 获取共享视图句柄
    pub fn series(&self) -> KlineSeries { ... }
    
    /// 获取下一个更新事件
    pub async fn next(&mut self) -> Option<KlineEvent> { ... }
}
```

## 4. 关键问题与边界处理
1. **时间对齐**: Quote 数据带有业务时间戳，K线的开盘/收盘时间桶应严格按照交易所规则和业务时间戳对齐，而不是本地机器时间。
2. **断线与重连**: 
   - `QuoteStream` 断线重连由底层 `WsConnection` 自动处理。
   - 断线期间如果错过了若干 quote 导致 K 线断层，重连后可以通过检查时间跨度决定是否需要重新调用一次 HTTP 补齐缺口（例如按最后一个已闭合 bar 的 bucket 边界补齐到当前时间桶）。
3. **缓存复用**:
   - 所有完结的 `KlineBar` (`is_completed = true`) 都会被写入本地 `HistoryCache`，下次任何模块申请相同 symbol+duration 数据时，直接走缓存。
