use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::price_data::OrderBookData, api_general::de_u64_from_string_or_number,
};

use crate::exchange::grvt::api_utils::grvt_ns_to_micros;

#[derive(Clone, Debug, Deserialize)]
pub struct RestOrderBookGrvt {
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    pub event_time: u64,
    pub instrument: String,
    pub bids: Vec<RestBookLevelGrvt>,
    pub asks: Vec<RestBookLevelGrvt>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RestBookLevelGrvt {
    pub price: String,
    pub size: String,
    pub num_orders: u64,
}

impl RestOrderBookGrvt {
    pub fn into_orderbook_data(self, inst: &str) -> OrderBookData {
        OrderBookData {
            timestamp: grvt_ns_to_micros(self.event_time),
            inst: inst.to_string(),
            bids: levels_to_pairs(self.bids),
            asks: levels_to_pairs(self.asks),
        }
    }
}

fn levels_to_pairs(levels: Vec<RestBookLevelGrvt>) -> Vec<(f64, f64)> {
    levels
        .into_iter()
        .map(|level| {
            (
                level.price.parse().unwrap_or_default(),
                level.size.parse().unwrap_or_default(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_book_levels() {
        let raw = r#"{"event_time":"1790586284450000000","instrument":"NVDA_USDT_Perp",
            "bids":[{"price":"224.09","size":"139.58","num_orders":1},{"price":"224.03","size":"0.07","num_orders":1},
                {"price":"223.99","size":"196.33","num_orders":1}],
            "asks":[{"price":"224.17","size":"50.51","num_orders":1},{"price":"224.2","size":"1.49","num_orders":1}]}"#;

        let book = serde_json::from_str::<RestOrderBookGrvt>(raw)
            .unwrap()
            .into_orderbook_data("NVDA_USDT_PERP");

        assert_eq!(book.inst, "NVDA_USDT_PERP");
        assert_eq!(book.timestamp, 1_790_586_284_450_000);
        assert_eq!(
            book.bids,
            vec![(224.09, 139.58), (224.03, 0.07), (223.99, 196.33)]
        );
        assert_eq!(book.asks, vec![(224.17, 50.51), (224.2, 1.49)]);
    }

    #[test]
    fn delisted_book_is_empty_and_untimed() {
        let book = serde_json::from_str::<RestOrderBookGrvt>(
            r#"{"event_time":"0","instrument":"IP_USDT_Perp","bids":[],"asks":[]}"#,
        )
        .unwrap()
        .into_orderbook_data("IP_USDT_PERP");

        assert!(book.bids.is_empty() && book.asks.is_empty());
        assert_eq!(book.timestamp, 0);
    }
}
