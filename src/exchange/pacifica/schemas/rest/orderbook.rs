use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::price_data::OrderBookData, api_general::ts_to_micros,
};

/// Up to ten aggregated levels a side, best price first; `l` is `[bids, asks]`.
#[derive(Clone, Debug, Deserialize)]
pub struct RestBookPacifica {
    pub s: String,
    pub l: (Vec<RestBookLevelPacifica>, Vec<RestBookLevelPacifica>),
    pub t: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RestBookLevelPacifica {
    pub p: String,
    pub a: String,
    pub n: u64,
}

impl RestBookPacifica {
    pub fn into_orderbook_data(self, inst: &str, depth: usize) -> OrderBookData {
        let (bids, asks) = self.l;

        OrderBookData {
            timestamp: ts_to_micros(self.t),
            inst: inst.to_string(),
            bids: levels_to_pairs(bids, depth),
            asks: levels_to_pairs(asks, depth),
        }
    }
}

fn levels_to_pairs(levels: Vec<RestBookLevelPacifica>, depth: usize) -> Vec<(f64, f64)> {
    levels
        .into_iter()
        .take(depth)
        .map(|level| {
            (
                level.p.parse().unwrap_or_default(),
                level.a.parse().unwrap_or_default(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = r#"{"s":"NVDA","l":[
        [{"p":"223.82","a":"0.086","n":1},{"p":"223.78","a":"5.146","n":2},{"p":"223.77","a":"21.231","n":2}],
        [{"p":"223.86","a":"11.618","n":1},{"p":"223.87","a":"0.057","n":1},{"p":"223.95","a":"16.734","n":1}]],
        "t":1790586168392}"#;

    fn book(depth: usize) -> OrderBookData {
        serde_json::from_str::<RestBookPacifica>(RAW)
            .unwrap()
            .into_orderbook_data("NVDA_USDC_PERP", depth)
    }

    #[test]
    fn parses_aggregated_book() {
        let book = book(10);

        assert_eq!(book.inst, "NVDA_USDC_PERP");
        assert_eq!(book.timestamp, 1_790_586_168_392_000);
        assert_eq!(
            book.bids,
            vec![(223.82, 0.086), (223.78, 5.146), (223.77, 21.231)]
        );
        assert_eq!(book.asks[0], (223.86, 11.618));
    }

    #[test]
    fn depth_caps_levels() {
        let book = book(1);

        assert_eq!((book.bids.len(), book.asks.len()), (1, 1));
    }

    #[test]
    fn book_without_both_sides_is_rejected() {
        assert!(
            serde_json::from_str::<RestBookPacifica>(r#"{"s":"NVDA","l":[[]],"t":1}"#).is_err()
        );
    }
}
