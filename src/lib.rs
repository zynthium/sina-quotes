pub mod history;
pub mod poll;
pub mod types;
pub mod ws;

pub use history::fetch_history;
pub use poll::poll_international;
pub use types::{Bar, Quote};
pub use ws::subscribe;

#[cfg(test)]
mod tests {
    use crate::types::{Bar, Quote};

    #[test]
    fn test_bar_default() {
        let bar = Bar {
            time: "2024-01-01".to_string(),
            open: 100.0,
            close: 105.0,
            high: 106.0,
            low: 99.0,
            volume: 1000000.0,
            open_interest: 245635.0,
        };
        assert_eq!(bar.time, "2024-01-01");
        assert_eq!(bar.close, 105.0);
    }

    #[test]
    fn test_quote_default() {
        let quote = Quote {
            symbol: "sh510050".to_string(),
            price: "2.930".to_string(),
            bid_price: String::new(),
            ask_price: String::new(),
            open: String::new(),
            high: String::new(),
            low: String::new(),
            prev_settle: String::new(),
            settle_price: String::new(),
            volume: "123456789".to_string(),
            quote_time: String::new(),
            date: String::new(),
            name: String::new(),
            timestamp: 1775486946,
        };
        assert_eq!(quote.symbol, "sh510050");
        assert_eq!(quote.price, "2.930");
    }
}