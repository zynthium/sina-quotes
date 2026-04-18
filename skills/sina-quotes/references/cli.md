# `sina-quotes` CLI 与示例

## 安装型 skill 的 GitHub 路径

```bash
npx skills add https://github.com/zynthium/sina-quotes --skill sina-quotes
```

## 构建

```bash
cargo build
```

## 历史 K 线

这是当前最适合直接用 CLI 做的一类任务：

```bash
cargo run -- klines hf_OIL 5
cargo run -- klines hf_GC 15 --count 200
```

参数含义：

- 第 1 个参数：外盘符号，例如 `hf_OIL`
- 第 2 个参数：分钟周期，例如 `5`、`15`
- `--count`：想输出的序列长度

## 交易时间段

```bash
# 查询交易时间段
cargo run -- market-hours SC0

# 也可以直接传其他期货 symbol
cargo run -- market-hours hf_FEF
```

这个命令只需要传 1 个 `symbol`，CLI 会根据它自动识别市场。

## 实时行情与实时 K 线

如果只是想快速验证实时 Quote，优先直接使用主 CLI：

```bash
cargo run -- subscribe hf_OIL
cargo run -- subscribe hf_OIL hf_GC
```

这个命令现在会真正启动 WebSocket、打印实时行情，并在空闲一段时间后自动退出。

如果想验证更细分的实时消费方式，再使用仓库里的 `examples/`。

单品种实时 Quote：

```bash
cargo run --example quote hf_OIL
```

多品种实时 Quote：

```bash
cargo run --example quotes_multi hf_OIL hf_GC
```

实时 1 分钟 K 线：

```bash
cargo run --example kline1m hf_OIL
```

同时消费 Quote 与实时 K 线：

```bash
cargo run --example combo hf_OIL
```

更完整的综合演示：

```bash
cargo run --example new_api history
cargo run --example new_api quote
cargo run --example new_api kline1m
cargo run --example new_api combo
cargo run --example new_api cache
```

## 什么时候优先用 CLI，什么时候优先用库 API

优先用 CLI / examples：

- 只想手动看一眼新浪外盘数据能不能拿到
- 只想快速确认某个国内 / 国外期货品种的交易时段
- 想快速验证 `hf_OIL`、`hf_CL`、`hf_GC` 这类符号是否可读
- 想做 demo、排查网络、观察输出格式
- 想快速验证主 CLI 的实时订阅链路是否正常

优先用库 API：

- 要把行情接到自己的 Rust 服务或策略程序里
- 要自己管理生命周期、缓存、超时和多品种订阅
- 要消费 `KlineSeries`、`QuoteStream`、`RealtimeKline` 这些结构化对象

## 常用外盘符号例子

```text
hf_OIL  布伦特原油
hf_CL   纽约原油
hf_NG   美国天然气
hf_GC   纽约黄金
hf_SI   纽约白银
hf_XAU  伦敦金
hf_XAG  伦敦银
```

如果不确定完整列表，优先在 Rust 代码里使用 `sina_quotes::symbols::ALL_SYMBOLS`。
