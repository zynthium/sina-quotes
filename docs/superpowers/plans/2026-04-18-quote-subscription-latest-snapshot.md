# Quote Subscription Latest Snapshot Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `subscribe_quote()` and `subscribe_quotes()` deliver a real latest quote immediately when one is already cached, and otherwise wait for the first real remote quote instead of emitting a placeholder event.

**Architecture:** Keep the public API unchanged and move the behavior change into `QuoteManager`. Each symbol keeps a `watch::Sender<Quote>` plus a `has_real_quote` flag. New subscribers attach to the existing watch channel; if a real quote is already cached, the new receiver is marked changed so its first `changed().await` returns immediately with the latest real snapshot, without waking older subscribers.

**Tech Stack:** Rust, Tokio `watch`, inline unit tests, `cargo test`

---

### Task 1: Lock the desired subscription semantics with tests

**Files:**
- Modify: `src/stream.rs`
- Test: `src/stream.rs`

- [ ] **Step 1: Write the failing tests**

```rust
#[tokio::test]
async fn test_subscribe_does_not_emit_placeholder_change() {
    let manager = QuoteManager::new();
    let mut stream = manager.subscribe("hf_CL").await;

    let res = tokio::time::timeout(std::time::Duration::from_millis(20), stream.changed()).await;

    assert!(res.is_err(), "placeholder event should not wake subscribers");
    assert_eq!(stream.borrow().symbol, "hf_CL");
    assert_eq!(stream.borrow().price, 0.0);
}

#[tokio::test]
async fn test_new_subscriber_immediately_receives_latest_real_quote() {
    let manager = QuoteManager::new();
    manager
        .update(Quote {
            symbol: "hf_CL".to_string(),
            price: 100.0,
            prev_settle: 95.0,
            ..Default::default()
        })
        .await;

    let mut stream = manager.subscribe("hf_CL").await;

    tokio::time::timeout(std::time::Duration::from_millis(20), stream.changed())
        .await
        .expect("latest real quote should be replayed")
        .expect("watch channel should stay open");

    let quote = stream.get();
    assert_eq!(quote.price, 100.0);
    assert_eq!(quote.prev_settle, 95.0);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_subscribe_does_not_emit_placeholder_change test_new_subscriber_immediately_receives_latest_real_quote -- --nocapture`
Expected: FAIL because `subscribe()` currently sends a placeholder `Quote`, and `update()` does not mark/replay a cached real quote for late subscribers.

- [ ] **Step 3: Write minimal implementation**

```rust
struct QuoteEntry {
    tx: watch::Sender<Quote>,
    has_real_quote: bool,
}
```

```rust
pub async fn subscribe(&self, symbol: &str) -> QuoteStream {
    let mut streams = self.streams.write().await;
    let entry = streams.entry(symbol.to_string()).or_insert_with(|| {
        let initial = Quote {
            symbol: symbol.to_string(),
            ..Default::default()
        };
        let (tx, _) = watch::channel(initial);
        QuoteEntry {
            tx,
            has_real_quote: false,
        }
    });

    let mut rx = entry.tx.subscribe();
    if entry.has_real_quote {
        rx.mark_changed();
    }
    QuoteStream::new(rx)
}

pub async fn update(&self, quote: Quote) {
    let mut streams = self.streams.write().await;
    let entry = streams.entry(quote.symbol.clone()).or_insert_with(|| {
        let (tx, _) = watch::channel(quote.clone());
        QuoteEntry {
            tx,
            has_real_quote: false,
        }
    });
    entry.has_real_quote = true;
    let _ = entry.tx.send(quote);
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_subscribe_does_not_emit_placeholder_change test_new_subscriber_immediately_receives_latest_real_quote -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/stream.rs docs/superpowers/plans/2026-04-18-quote-subscription-latest-snapshot.md
git commit -m "fix: replay cached quote to new subscribers"
```

### Task 2: Prove the behavior still works for existing subscribers and multi-symbol usage

**Files:**
- Modify: `src/stream.rs`
- Test: `src/stream.rs`

- [ ] **Step 1: Extend tests around existing behavior**

```rust
#[tokio::test]
async fn test_existing_subscriber_receives_future_updates() {
    let manager = QuoteManager::new();
    let mut stream = manager.subscribe("hf_CL").await;

    manager
        .update(Quote {
            symbol: "hf_CL".to_string(),
            price: 101.0,
            prev_settle: 96.0,
            ..Default::default()
        })
        .await;

    stream.changed().await.unwrap();
    let quote = stream.get();
    assert_eq!(quote.price, 101.0);
    assert_eq!(quote.prev_settle, 96.0);
}
```

- [ ] **Step 2: Run the focused stream tests**

Run: `cargo test stream::tests -- --nocapture`
Expected: PASS

- [ ] **Step 3: Refactor only if needed**

```rust
fn real_quote_sender(symbol: &str) -> (watch::Sender<Quote>, watch::Receiver<Quote>) {
    let initial = Quote {
        symbol: symbol.to_string(),
        ..Default::default()
    };
    watch::channel(initial)
}
```

- [ ] **Step 4: Run the focused stream tests again**

Run: `cargo test stream::tests -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/stream.rs
git commit -m "test: cover cached quote subscription semantics"
```

### Task 3: Verify the behavior from the public API surface

**Files:**
- Modify: `examples/quote.rs` (only if extra clarity is useful)
- Test: `src/client.rs` or `src/stream.rs`

- [ ] **Step 1: Add a public-surface regression test if needed**

```rust
#[tokio::test]
async fn test_subscribe_quote_waits_for_real_quote_then_replays_cached_value() {
    let client = SinaQuotes::new().await.unwrap();
    let mut stream = client.subscribe_quote("hf_TEST").await.unwrap();

    let res = tokio::time::timeout(std::time::Duration::from_millis(20), stream.changed()).await;
    assert!(res.is_err());
}
```

- [ ] **Step 2: Run the targeted public API tests**

Run: `cargo test subscribe_quote -- --nocapture`
Expected: PASS

- [ ] **Step 3: Update the example output if desired**

```rust
println!(
    "[Q{:02}] {} prev_settle={:.3}",
    i,
    q,
    q.prev_settle
);
```

- [ ] **Step 4: Run formatting and the relevant tests**

Run: `cargo fmt`
Run: `cargo test stream::tests -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/client.rs src/stream.rs examples/quote.rs
git commit -m "docs: clarify first real quote subscription behavior"
```
