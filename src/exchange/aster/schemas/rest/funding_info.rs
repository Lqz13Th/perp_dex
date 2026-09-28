use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::utils_data::FundingRateInfo, api_general::ts_to_micros,
};

use crate::exchange::aster::api_utils::aster_inst_to_cli;

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct RestFundingInfoAster {
    pub symbol: String,
    pub interestRate: String,
    pub time: u64,
    pub fundingIntervalHours: u64,
    pub fundingFeeCap: f64,
    pub fundingFeeFloor: f64,
}

impl From<RestFundingInfoAster> for FundingRateInfo {
    fn from(d: RestFundingInfoAster) -> Self {
        FundingRateInfo {
            timestamp: ts_to_micros(d.time),
            inst: aster_inst_to_cli(&d.symbol),
            funding_interval_sec: (d.fundingIntervalHours * 3600) as f64,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn funding_interval_is_in_seconds() {
        let raw = r#"{"symbol":"TRUTHUSDT","interestRate":"0.00010000","time":1790578199000,
            "fundingIntervalHours":4,"fundingFeeCap":0.02,"fundingFeeFloor":-0.02}"#;
        let info =
            FundingRateInfo::from(serde_json::from_str::<RestFundingInfoAster>(raw).unwrap());

        assert_eq!(info.inst, "TRUTH_USDT_PERP");
        assert_eq!(info.funding_interval_sec, 14_400.0);
        assert_eq!(info.timestamp, 1_790_578_199_000_000);
    }
}
