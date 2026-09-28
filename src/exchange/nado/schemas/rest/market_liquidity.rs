use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::price_data::OrderBookData, api_general::de_u64_from_string_or_number,
};

use crate::exchange::nado::api_utils::{nado_ns_to_micros, nado_product_to_cli, nado_x18_to_f64};

/// `[price_x18, size_x18]` levels, best first; `timestamp` is in nanoseconds.
#[derive(Clone, Debug, Deserialize)]
pub struct RestMarketLiquidityNado {
    pub bids: Vec<[String; 2]>,
    pub asks: Vec<[String; 2]>,
    pub product_id: u32,
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    pub timestamp: u64,
}

impl RestMarketLiquidityNado {
    pub fn into_orderbook_data(self) -> OrderBookData {
        OrderBookData {
            timestamp: nado_ns_to_micros(self.timestamp),
            inst: nado_product_to_cli(self.product_id),
            bids: levels_to_pairs(self.bids),
            asks: levels_to_pairs(self.asks),
        }
    }
}

fn levels_to_pairs(levels: Vec<[String; 2]>) -> Vec<(f64, f64)> {
    levels
        .iter()
        .map(|[price, size]| (nado_x18_to_f64(price), nado_x18_to_f64(size)))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::exchange::nado::nado_rest_msg::RestResNado;
    use extrema_infra::prelude::IntoInfraData;

    use super::*;

    #[test]
    fn parses_market_liquidity() {
        let raw = r#"{"status":"success","data":{"bids":[["223920000000000000000","2230000000000000000"],
            ["223900000000000000000","830000000000000000"],["223890000000000000000","1110000000000000000"]],
            "asks":[["223970000000000000000","670000000000000000"],["223980000000000000000","670000000000000000"],
            ["223990000000000000000","460000000000000000"]],"product_id":112,"timestamp":"1790586268388285932"},
            "request_type":"query_market_liquidity"}"#;

        let liquidity = serde_json::from_str::<RestResNado<RestMarketLiquidityNado>>(raw)
            .unwrap()
            .into_one()
            .unwrap();
        assert_eq!(liquidity.timestamp, 1_790_586_268_388_285_932);

        let book = liquidity.into_orderbook_data();
        assert_eq!(book.inst, "@112");
        assert_eq!(book.timestamp, 1_790_586_268_388_285);
        assert_eq!(
            book.bids,
            vec![(223.92, 2.23), (223.9, 0.83), (223.89, 1.11)]
        );
        assert_eq!(book.asks[0], (223.97, 0.67));
        assert_eq!(book.asks.len(), 3);
    }

    #[test]
    fn empty_book_is_empty() {
        let book = serde_json::from_str::<RestMarketLiquidityNado>(
            r#"{"bids":[],"asks":[],"product_id":2,"timestamp":"1790585665589483524"}"#,
        )
        .unwrap()
        .into_orderbook_data();

        assert!(book.bids.is_empty() && book.asks.is_empty());
        assert_eq!(book.inst, "@2");
    }
}
