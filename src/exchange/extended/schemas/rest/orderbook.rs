use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::price_data::OrderBookData, api_general::get_micros_timestamp,
};

/// The whole book, best price first; it carries no exchange time.
#[derive(Clone, Debug, Deserialize)]
pub struct RestOrderBookExtended {
    pub market: String,
    pub bid: Vec<RestBookLevelExtended>,
    pub ask: Vec<RestBookLevelExtended>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RestBookLevelExtended {
    pub qty: String,
    pub price: String,
}

impl RestOrderBookExtended {
    pub fn into_orderbook_data(self, inst: &str, depth: usize) -> OrderBookData {
        OrderBookData {
            timestamp: get_micros_timestamp(),
            inst: inst.to_string(),
            bids: levels_to_pairs(self.bid, depth),
            asks: levels_to_pairs(self.ask, depth),
        }
    }
}

fn levels_to_pairs(levels: Vec<RestBookLevelExtended>, depth: usize) -> Vec<(f64, f64)> {
    let depth = if depth == 0 { levels.len() } else { depth };

    levels
        .into_iter()
        .take(depth)
        .map(|level| {
            (
                level.price.parse().unwrap_or_default(),
                level.qty.parse().unwrap_or_default(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = r#"{"market":"NVDA_24_5-USD","bid":[{"qty":"0.64","price":"223.69"},
        {"qty":"44.29","price":"223.68"},{"qty":"116.18","price":"223.62"}],
        "ask":[{"qty":"0.66","price":"223.94"},{"qty":"0.81","price":"223.95"}]}"#;

    fn book(depth: usize) -> OrderBookData {
        serde_json::from_str::<RestOrderBookExtended>(RAW)
            .unwrap()
            .into_orderbook_data("NVDA_24_5_USD_PERP", depth)
    }

    #[test]
    fn parses_the_whole_book() {
        let book = book(0);

        assert_eq!(book.inst, "NVDA_24_5_USD_PERP");
        assert_eq!(
            book.bids,
            vec![(223.69, 0.64), (223.68, 44.29), (223.62, 116.18)]
        );
        assert_eq!(book.asks, vec![(223.94, 0.66), (223.95, 0.81)]);
        assert!(book.timestamp > 1_700_000_000_000_000);
    }

    #[test]
    fn depth_keeps_the_best_levels() {
        let book = book(1);

        assert_eq!(book.bids, vec![(223.69, 0.64)]);
        assert_eq!(book.asks, vec![(223.94, 0.66)]);
    }
}
