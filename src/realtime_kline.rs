use crate::data::types::{Duration, KlineBar, Quote};

use crate::data::series::KlineSeries;
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct KlineEvent {
    pub bar: KlineBar,
    pub is_completed: bool,
}

pub struct KlineAggregator {
    duration: Duration,
    current_bucket_id: Option<i64>,
    current: Option<KlineBar>,
    last_total_volume: Option<f64>,
}

pub struct RealtimeKline {
    series: KlineSeries,
    rx: broadcast::Receiver<KlineEvent>,
}

impl RealtimeKline {
    pub fn new(series: KlineSeries, rx: broadcast::Receiver<KlineEvent>) -> Self {
        Self { series, rx }
    }

    pub fn series(&self) -> KlineSeries {
        self.series.clone()
    }

    pub async fn next(&mut self) -> Option<KlineEvent> {
        loop {
            match self.rx.recv().await {
                Ok(ev) => return Some(ev),
                Err(broadcast::error::RecvError::Closed) => return None,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
    }
}

impl KlineAggregator {
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            current_bucket_id: None,
            current: None,
            last_total_volume: None,
        }
    }

    pub fn seed_from_bar(&mut self, bar: KlineBar) {
        self.current_bucket_id = Some(bar.id);
        self.current = Some(bar);
    }

    pub fn on_quote(&mut self, quote: Quote) -> Vec<KlineEvent> {
        let dur_secs = self.duration.as_secs() as i64;
        if dur_secs <= 0 {
            return Vec::new();
        }

        let bucket_id = quote.timestamp.div_euclid(dur_secs);

        if let Some(current_bucket_id) = self.current_bucket_id
            && bucket_id < current_bucket_id
        {
            return Vec::new();
        }

        let delta_volume = match self.last_total_volume {
            Some(prev) if quote.volume >= prev => quote.volume - prev,
            _ => 0.0,
        };
        self.last_total_volume = Some(quote.volume);

        let bucket_start_ns = bucket_id * dur_secs * 1_000_000_000;

        if self.current_bucket_id.is_none() {
            let bar = KlineBar {
                id: bucket_id,
                datetime: bucket_start_ns,
                open: quote.price,
                high: quote.price,
                low: quote.price,
                close: quote.price,
                volume: delta_volume,
                open_interest: 0.0,
            };
            self.current_bucket_id = Some(bucket_id);
            self.current = Some(bar.clone());
            return vec![KlineEvent {
                bar,
                is_completed: false,
            }];
        }

        let current_bucket_id = self.current_bucket_id.unwrap();
        if bucket_id == current_bucket_id {
            let mut bar = self.current.clone().unwrap();
            bar.high = bar.high.max(quote.price);
            bar.low = bar.low.min(quote.price);
            bar.close = quote.price;
            bar.volume += delta_volume;
            self.current = Some(bar.clone());
            return vec![KlineEvent {
                bar,
                is_completed: false,
            }];
        }

        let mut events = Vec::with_capacity(2);
        if let Some(prev) = self.current.take() {
            events.push(KlineEvent {
                bar: prev,
                is_completed: true,
            });
        }

        let bar = KlineBar {
            id: bucket_id,
            datetime: bucket_start_ns,
            open: quote.price,
            high: quote.price,
            low: quote.price,
            close: quote.price,
            volume: delta_volume,
            open_interest: 0.0,
        };

        self.current_bucket_id = Some(bucket_id);
        self.current = Some(bar.clone());
        events.push(KlineEvent {
            bar,
            is_completed: false,
        });
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::series::KlineSeries;

    #[test]
    fn test_bucket_rollover_emits_completed_and_current() {
        let dur = Duration::minutes(1);
        let mut agg = KlineAggregator::new(dur);

        let q1 = Quote {
            symbol: "hf_TEST".into(),
            price: 10.0,
            volume: 100.0,
            timestamp: 60,
            ..Default::default()
        };
        let q2 = Quote {
            symbol: "hf_TEST".into(),
            price: 12.0,
            volume: 105.0,
            timestamp: 61,
            ..Default::default()
        };
        let q3 = Quote {
            symbol: "hf_TEST".into(),
            price: 11.0,
            volume: 110.0,
            timestamp: 120,
            ..Default::default()
        };

        let e1 = agg.on_quote(q1);
        assert_eq!(e1.len(), 1);
        assert!(!e1[0].is_completed);

        let e2 = agg.on_quote(q2);
        assert_eq!(e2.len(), 1);
        assert!(!e2[0].is_completed);

        let e3 = agg.on_quote(q3);
        assert_eq!(e3.len(), 2);
        assert!(e3[0].is_completed);
        assert!(!e3[1].is_completed);
    }

    #[tokio::test]
    async fn test_realtime_kline_next_receives_events() {
        let (tx, rx) = broadcast::channel(16);
        let series = KlineSeries::new("hf_TEST".to_string(), Duration::minutes(1), 10);
        let mut sub = RealtimeKline::new(series.clone(), rx);

        tx.send(KlineEvent {
            bar: KlineBar {
                id: 1,
                datetime: 0,
                open: 1.0,
                high: 1.0,
                low: 1.0,
                close: 1.0,
                volume: 0.0,
                open_interest: 0.0,
            },
            is_completed: false,
        })
        .unwrap();

        let ev = sub.next().await.unwrap();
        assert_eq!(ev.bar.id, 1);
        assert!(!ev.is_completed);
        assert_eq!(sub.series().symbol(), "hf_TEST");
    }

    #[test]
    fn test_seed_from_bar_updates_same_bucket() {
        let dur = Duration::minutes(1);
        let mut agg = KlineAggregator::new(dur);
        agg.seed_from_bar(KlineBar {
            id: 10,
            datetime: 10 * 60 * 1_000_000_000,
            open: 5.0,
            high: 6.0,
            low: 4.0,
            close: 5.0,
            volume: 0.0,
            open_interest: 0.0,
        });

        let q = Quote {
            symbol: "hf_TEST".into(),
            price: 7.0,
            volume: 100.0,
            timestamp: 10 * 60 + 1,
            ..Default::default()
        };

        let evs = agg.on_quote(q);
        assert_eq!(evs.len(), 1);
        assert_eq!(evs[0].bar.open, 5.0);
        assert_eq!(evs[0].bar.close, 7.0);
    }
}
