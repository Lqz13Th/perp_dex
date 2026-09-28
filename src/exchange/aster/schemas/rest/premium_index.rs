use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::{price_data::MarkPriceData, utils_data::FundingRateData},
    api_general::ts_to_micros,
    base_data::InstrumentType,
};

use crate::exchange::aster::api_utils::aster_inst_to_cli;

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct RestPremiumIndexAster {
    pub symbol: String,
    pub markPrice: String,
    pub indexPrice: String,
    pub estimatedSettlePrice: String,
    pub lastFundingRate: String,
    pub interestRate: String,
    pub nextFundingTime: u64,
    pub time: u64,
}

impl From<RestPremiumIndexAster> for FundingRateData {
    fn from(d: RestPremiumIndexAster) -> Self {
        FundingRateData {
            timestamp: ts_to_micros(d.time),
            inst: aster_inst_to_cli(&d.symbol),
            funding_rate: d.lastFundingRate.parse().unwrap_or_default(),
            funding_time: ts_to_micros(d.nextFundingTime),
        }
    }
}

impl From<RestPremiumIndexAster> for MarkPriceData {
    fn from(d: RestPremiumIndexAster) -> Self {
        MarkPriceData {
            timestamp: ts_to_micros(d.time),
            inst: aster_inst_to_cli(&d.symbol),
            inst_type: InstrumentType::Perpetual,
            mark_price: d.markPrice.parse().unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = r#"{"symbol":"NVDAUSDT","markPrice":"223.51856917","indexPrice":"223.30879286",
        "estimatedSettlePrice":"223.4","lastFundingRate":"0.00000906","interestRate":"0",
        "nextFundingTime":1790582400000,"time":1790577908000}"#;

    #[test]
    fn premium_index_maps_to_mark_price_and_funding() {
        let raw: RestPremiumIndexAster = serde_json::from_str(RAW).unwrap();
        let mark = MarkPriceData::from(raw.clone());
        let funding = FundingRateData::from(raw);

        assert_eq!(mark.inst, "NVDA_USDT_PERP");
        assert_eq!(mark.mark_price, 223.51856917);
        assert_eq!(mark.timestamp, 1_790_577_908_000_000);
        assert_eq!(funding.funding_rate, 0.00000906);
        assert_eq!(funding.funding_time, 1_790_582_400_000_000);
    }
}
