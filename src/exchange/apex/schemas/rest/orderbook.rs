use serde::Deserialize;

use extrema_infra::{
    arch::market_assets::{api_data::price_data::OrderBookData, api_general::value_to_f64},
    prelude::{InfraError, InfraResult},
};

/// ApeX answers an unknown symbol with `"s":""` and null sides; the depth carries no exchange time.
#[derive(Clone, Debug, Deserialize)]
pub struct RestOrderBookApex {
    pub s: String,
    pub u: u64,
    pub b: Option<Vec<[serde_json::Value; 2]>>,
    pub a: Option<Vec<[serde_json::Value; 2]>>,
}

impl RestOrderBookApex {
    pub fn into_orderbook_data(self, inst: &str, timestamp: u64) -> InfraResult<OrderBookData> {
        let (Some(bids), Some(asks)) = (self.b, self.a) else {
            return Err(InfraError::ApiCliError(format!(
                "ApeX has no order book for {inst}"
            )));
        };

        Ok(OrderBookData {
            timestamp,
            inst: inst.to_string(),
            bids: levels_to_pairs(bids),
            asks: levels_to_pairs(asks),
        })
    }
}

fn levels_to_pairs(levels: Vec<[serde_json::Value; 2]>) -> Vec<(f64, f64)> {
    levels
        .into_iter()
        .map(|[price, size]| (value_to_f64(&price), value_to_f64(&size)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_depth_snapshot() {
        let raw = r#"{"a":[["223.30","253.38"],["223.32","2.88"],["223.35","249.15"]],
            "b":[["223.18","218.85"],["223.16","1.53"]],"s":"NVDAUSDT","u":696085}"#;

        let book = serde_json::from_str::<RestOrderBookApex>(raw)
            .unwrap()
            .into_orderbook_data("NVDA_USDT_PERP", 9)
            .unwrap();

        assert_eq!(book.inst, "NVDA_USDT_PERP");
        assert_eq!(book.timestamp, 9);
        assert_eq!(book.bids, vec![(223.18, 218.85), (223.16, 1.53)]);
        assert_eq!(book.asks[0], (223.3, 253.38));
        assert_eq!(book.asks.len(), 3);
    }

    #[test]
    fn unknown_symbol_is_an_error() {
        let err = serde_json::from_str::<RestOrderBookApex>(r#"{"a":null,"b":null,"s":"","u":0}"#)
            .unwrap()
            .into_orderbook_data("NOPE_USDT_PERP", 1)
            .unwrap_err();

        assert!(err.to_string().contains("NOPE_USDT_PERP"), "{err}");
    }
}
