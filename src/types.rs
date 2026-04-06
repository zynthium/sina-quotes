use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bar {
    pub time: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    pub open_interest: f64,
}

/// Real-time quote for international futures
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Quote {
    pub symbol: String,
    pub price: String,
    pub bid_price: String,
    pub ask_price: String,
    pub open: String,
    pub high: String,
    pub low: String,
    pub prev_settle: String,
    pub settle_price: String,
    pub volume: String,
    pub quote_time: String,
    pub date: String,
    pub name: String,
    pub timestamp: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bar_serialize() {
        let bar = Bar {
            time: "2024-01-01".to_string(),
            open: 100.0,
            high: 106.0,
            low: 99.0,
            close: 105.0,
            volume: 1000000.0,
            open_interest: 245635.0,
        };
        let json = serde_json::to_string(&bar).unwrap();
        assert!(json.contains("2024-01-01"));
        assert!(json.contains("100.0"));
    }

    #[test]
    fn test_quote_serialize() {
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
        let json = serde_json::to_string(&quote).unwrap();
        assert!(json.contains("sh510050"));
        assert!(json.contains("2.930"));
    }

    #[test]
    fn test_quote_deserialize() {
        let json = r#"{"symbol":"sh510050","price":"2.930","bid_price":"","ask_price":"","open":"","high":"","low":"","prev_settle":"","settle_price":"","volume":"123456789","quote_time":"","date":"","name":"","timestamp":1775486946}"#;
        let quote: Quote = serde_json::from_str(json).unwrap();
        assert_eq!(quote.symbol, "sh510050");
        assert_eq!(quote.price, "2.930");
    }
}
