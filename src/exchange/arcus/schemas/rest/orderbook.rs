use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::price_data::OrderBookData,
    api_general::{ts_to_micros, value_to_f64},
};

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct RestOrderBookArcus {
    pub bids: Vec<[serde_json::Value; 2]>,
    pub asks: Vec<[serde_json::Value; 2]>,
    pub lastSequenceId: u64,
    pub timestamp: u64,
}

impl RestOrderBookArcus {
    pub fn into_orderbook_data(self, inst: &str) -> OrderBookData {
        OrderBookData {
            timestamp: ts_to_micros(self.timestamp),
            inst: inst.to_string(),
            bids: levels_to_pairs(self.bids),
            asks: levels_to_pairs(self.asks),
        }
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
    fn parses_l2_snapshot() {
        let raw = r#"{"bids":[["223.38","1.4325364"],["223.37","0.4456327"]],
            "asks":[["223.4","2.238"]],"lastSequenceId":53020789,"globalSequenceId":2615218764,
            "timestamp":1790577885964041}"#;

        let book = serde_json::from_str::<RestOrderBookArcus>(raw)
            .unwrap()
            .into_orderbook_data("NVDA_USD_PERP");

        assert_eq!(book.inst, "NVDA_USD_PERP");
        assert_eq!(book.timestamp, 1_790_577_885_964_041);
        assert_eq!(book.bids, vec![(223.38, 1.4325364), (223.37, 0.4456327)]);
        assert_eq!(book.asks, vec![(223.4, 2.238)]);
    }
}
