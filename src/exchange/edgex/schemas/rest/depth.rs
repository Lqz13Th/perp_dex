use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::price_data::OrderBookData, api_general::de_u64_from_string_or_number,
};

use crate::exchange::edgex::api_utils::edgex_contract_to_cli;

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct RestDepthEdgex {
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    pub startVersion: u64,
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    pub endVersion: u64,
    pub level: u16,
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    pub contractId: u64,
    pub contractName: String,
    pub asks: Vec<RestBookLevelEdgex>,
    pub bids: Vec<RestBookLevelEdgex>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RestBookLevelEdgex {
    pub price: String,
    pub size: String,
}

impl RestDepthEdgex {
    pub fn into_orderbook_data(self, timestamp: u64) -> OrderBookData {
        OrderBookData {
            timestamp,
            inst: edgex_contract_to_cli(self.contractId),
            bids: levels_to_pairs(self.bids),
            asks: levels_to_pairs(self.asks),
        }
    }
}

fn levels_to_pairs(levels: Vec<RestBookLevelEdgex>) -> Vec<(f64, f64)> {
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
    fn parses_depth_snapshot() {
        let raw = r#"{"startVersion":"1109000640","endVersion":"1109000670","level":15,
            "contractId":"30000020","contractName":"NVDAUSDC",
            "asks":[{"price":"223.45","size":"15.40"},{"price":"223.46","size":"17.75"}],
            "bids":[{"price":"223.30","size":"13.78"},{"price":"223.29","size":"20.72"},
                    {"price":"223.28","size":"22.60"}],"depthType":"SNAPSHOT"}"#;

        let depth = serde_json::from_str::<RestDepthEdgex>(raw).unwrap();
        assert_eq!(
            (depth.startVersion, depth.endVersion, depth.level),
            (1109000640, 1109000670, 15)
        );

        let book = depth.into_orderbook_data(1_790_585_440_700_000);
        assert_eq!(book.inst, "@30000020");
        assert_eq!(book.timestamp, 1_790_585_440_700_000);
        assert_eq!(book.asks, vec![(223.45, 15.4), (223.46, 17.75)]);
        assert_eq!(book.bids[0], (223.3, 13.78));
        assert_eq!(book.bids.len(), 3);
    }
}
