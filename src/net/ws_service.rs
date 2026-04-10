//! WebSocket 连接管理器
//!
//! 管理 WebSocket 连接，支持自动重连。

use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use tokio::sync::RwLock;
use tokio::time::sleep;

use crate::net::ws;
use crate::stream::{QuoteFeedManager, QuoteManager};

/// WebSocket 连接状态
#[derive(Debug, Clone)]
pub enum WsState {
    Connecting,
    Connected,
    Reconnecting { attempt: u32 },
    Disconnected,
}

/// WebSocket 连接管理器
#[derive(Clone)]
pub struct WsConnection {
    symbols: Vec<String>,
    state: Arc<RwLock<WsState>>,
    quote_manager: QuoteManager,
    quote_feed_manager: QuoteFeedManager,
    closed: Arc<RwLock<bool>>,
    reconnect_delay: Duration,
    max_attempts: u32,
}

impl WsConnection {
    pub fn new(
        symbols: Vec<String>,
        quote_manager: QuoteManager,
        quote_feed_manager: QuoteFeedManager,
        reconnect_delay: Duration,
        max_attempts: u32,
    ) -> Self {
        Self {
            symbols,
            state: Arc::new(RwLock::new(WsState::Disconnected)),
            quote_manager,
            quote_feed_manager,
            closed: Arc::new(RwLock::new(false)),
            reconnect_delay,
            max_attempts,
        }
    }

    async fn subscribe_stream(
        symbols: &[&str],
        is_international: bool,
    ) -> std::result::Result<
        Pin<
            Box<
                dyn futures_util::Stream<
                        Item = std::result::Result<crate::data::types::Quote, ws::Error>,
                    > + Send,
            >,
        >,
        ws::Error,
    > {
        if is_international {
            let stream = ws::subscribe_international(symbols).await?;
            Ok(Box::pin(stream))
        } else {
            let stream = ws::subscribe(symbols).await?;
            Ok(Box::pin(stream))
        }
    }

    #[allow(clippy::never_loop)]
    pub async fn start(&self) {
        let symbols_refs: Vec<&str> = self.symbols.iter().map(|s| s.as_str()).collect();
        let is_international = symbols_refs.iter().any(|s| s.starts_with("hf_"));

        loop {
            if *self.closed.read().await {
                break;
            }

            *self.state.write().await = WsState::Connecting;

            match Self::subscribe_stream(&symbols_refs, is_international).await {
                Ok(mut stream) => {
                    *self.state.write().await = WsState::Connected;
                    tracing::info!("WebSocket connected: {:?}", self.symbols);

                    while let Some(result) = stream.next().await {
                        if *self.closed.read().await {
                            break;
                        }

                        match result {
                            Ok(quote) => {
                                self.quote_manager.update(quote.clone()).await;
                                self.quote_feed_manager.publish(quote).await;
                            }
                            Err(e) => {
                                tracing::warn!("WebSocket error: {}", e);
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("WebSocket connection failed: {}", e);
                }
            }

            if *self.closed.read().await {
                break;
            }

            let mut attempt = 1;
            *self.state.write().await = WsState::Reconnecting { attempt };

            while attempt <= self.max_attempts {
                if *self.closed.read().await {
                    return;
                }

                tracing::info!(
                    "Reconnecting in {:?} (attempt {}/{})",
                    self.reconnect_delay,
                    attempt,
                    self.max_attempts
                );

                sleep(self.reconnect_delay).await;

                if *self.closed.read().await {
                    return;
                }

                *self.state.write().await = WsState::Reconnecting { attempt };

                match Self::subscribe_stream(&symbols_refs, is_international).await {
                    Ok(mut stream) => {
                        *self.state.write().await = WsState::Connected;
                        tracing::info!("WebSocket reconnected: {:?}", self.symbols);

                        while let Some(result) = stream.next().await {
                            if *self.closed.read().await {
                                break;
                            }

                            match result {
                                Ok(quote) => {
                                    self.quote_manager.update(quote.clone()).await;
                                    self.quote_feed_manager.publish(quote).await;
                                }
                                Err(e) => {
                                    tracing::warn!("WebSocket error: {}", e);
                                    break;
                                }
                            }
                        }

                        attempt += 1;
                    }
                    Err(e) => {
                        tracing::warn!("Reconnection failed: {}", e);
                        attempt += 1;
                    }
                }
            }

            tracing::error!(
                "Max reconnection attempts ({}) reached for {:?}",
                self.max_attempts,
                self.symbols
            );
            break;
        }

        *self.state.write().await = WsState::Disconnected;
    }

    pub async fn state(&self) -> WsState {
        self.state.read().await.clone()
    }

    pub async fn close(&self) {
        *self.closed.write().await = true;
        *self.state.write().await = WsState::Disconnected;
    }

    pub async fn is_closed(&self) -> bool {
        *self.closed.read().await
    }
}

impl std::fmt::Debug for WsConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WsConnection")
            .field("symbols", &self.symbols)
            .field("closed", &self.closed)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ws_state() {
        let quote_manager = QuoteManager::new();
        let conn = WsConnection::new(
            vec!["hf_CL".to_string()],
            quote_manager,
            QuoteFeedManager::new(),
            Duration::from_secs(1),
            3,
        );

        assert!(!conn.is_closed().await);

        conn.close().await;
        assert!(conn.is_closed().await);
    }
}
