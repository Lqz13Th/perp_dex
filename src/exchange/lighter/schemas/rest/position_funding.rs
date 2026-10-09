use serde::Deserialize;

use crate::exchange::lighter::{
    api_utils::lighter_market_to_cli, schemas::rest::open_order::to_micros,
};

/// `GET /api/v1/positionFunding` (auth token in the `authorization` header).
#[derive(Clone, Debug, Deserialize)]
pub struct RestPositionFundingLighter {
    #[serde(default)]
    pub position_fundings: Vec<PositionFundingLighter>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// One funding payment of one position; `change` is the USDC credited (negative when paid).
#[derive(Clone, Debug, Deserialize)]
pub struct PositionFundingLighter {
    pub timestamp: u64,
    pub market_id: u16,
    pub funding_id: i64,
    pub change: String,
    #[serde(default)]
    pub discount: String,
    pub rate: String,
    pub position_size: String,
    /// `long` or `short`
    pub position_side: String,
}

impl PositionFundingLighter {
    pub fn inst(&self) -> String {
        lighter_market_to_cli(self.market_id)
    }

    pub fn timestamp_us(&self) -> u64 {
        to_micros(self.timestamp)
    }

    pub fn change_usdc(&self) -> f64 {
        self.change.parse().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::lighter::lighter_rest_msg::RestResLighter;
    use extrema_infra::prelude::IntoInfraData;

    #[test]
    fn funding_payments_parse() {
        let r: RestResLighter<RestPositionFundingLighter> = serde_json::from_str(
            r#"{"code":200,"position_fundings":[{"timestamp":1791525600,"market_id":139,"funding_id":4721,
            "change":"-0.001840","discount":"0.000000","rate":"0.0000158","position_size":"0.0071",
            "position_side":"long"}],"next_cursor":"eyJp"}"#,
        )
        .unwrap();
        let page = r.into_one().unwrap();
        let f = &page.position_fundings[0];
        assert_eq!(
            (f.inst().as_str(), f.timestamp_us()),
            ("@139", 1791525600000000)
        );
        assert!((f.change_usdc() + 0.00184).abs() < 1e-12);
        assert_eq!(page.next_cursor.as_deref(), Some("eyJp"));

        let empty: RestResLighter<RestPositionFundingLighter> =
            serde_json::from_str(r#"{"code":200,"position_fundings":[]}"#).unwrap();
        assert!(empty.into_one().unwrap().position_fundings.is_empty());
    }
}
