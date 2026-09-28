use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::price_data::OrderBookData,
    api_general::{ts_to_micros, value_to_f64},
};

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct RestOrderBookAster {
    pub lastUpdateId: u64,
    pub T: u64,
    pub bids: Vec<[serde_json::Value; 2]>,
    pub asks: Vec<[serde_json::Value; 2]>,
}

impl RestOrderBookAster {
    pub fn into_orderbook_data(self, inst: &str) -> OrderBookData {
        OrderBookData {
            timestamp: ts_to_micros(self.T),
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
    fn parses_depth_snapshot() {
        let raw = r#"{"lastUpdateId":570480090871,"E":1790577884824,"T":1790577884800,
            "bids":[["223.510000","1.03"],["223.490000","2.69"]],
            "asks":[["223.570000","2.22"]]}"#;

        let book = serde_json::from_str::<RestOrderBookAster>(raw)
            .unwrap()
            .into_orderbook_data("NVDA_USDT_PERP");

        assert_eq!(book.inst, "NVDA_USDT_PERP");
        assert_eq!(book.timestamp, 1_790_577_884_800_000);
        assert_eq!(book.bids, vec![(223.51, 1.03), (223.49, 2.69)]);
        assert_eq!(book.asks, vec![(223.57, 2.22)]);
    }
}
