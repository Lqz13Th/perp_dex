use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::price_data::{MarkPriceData, TickerData},
    api_general::de_u64_from_string_or_number,
    base_data::InstrumentType,
};

use crate::exchange::edgex::api_utils::edgex_contract_to_cli;

/// 24h ticker of one contract; `lastPrice` stays 0 until its first trade.
#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct RestTickerEdgex {
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    pub contractId: u64,
    pub contractName: String,
    pub lastPrice: String,
    pub markPrice: String,
    pub indexPrice: String,
    pub oraclePrice: String,
    pub openInterest: String,
    pub size: String,
    pub value: String,
    pub fundingRate: String,
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    pub nextFundingTime: u64,
}

impl RestTickerEdgex {
    pub fn inst(&self) -> String {
        edgex_contract_to_cli(self.contractId)
    }

    pub fn into_ticker_data(self, timestamp: u64) -> Option<TickerData> {
        let price = self.lastPrice.parse().ok().filter(|price| *price > 0.0)?;

        Some(TickerData {
            timestamp,
            inst: self.inst(),
            inst_type: InstrumentType::Perpetual,
            price,
        })
    }

    pub fn into_mark_price_data(self, timestamp: u64) -> Option<MarkPriceData> {
        let mark_price = self.markPrice.parse().ok().filter(|price| *price > 0.0)?;

        Some(MarkPriceData {
            timestamp,
            inst: self.inst(),
            inst_type: InstrumentType::Perpetual,
            mark_price,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NVDA: &str = r#"{"contractId":"30000020","contractName":"NVDAUSDC","priceChange":"-1.66",
        "priceChangePercent":"-0.007375","trades":"4806","size":"2309.03","value":"518556.6254",
        "high":"225.85","low":"223.01","open":"225.07","close":"223.41","highTime":"1790555043549",
        "lowTime":"1790584794580","startTime":"1790497800000","endTime":"1790586000000",
        "lastPrice":"223.41","indexPrice":"223.9542120649","oraclePrice":"224.025606310472291561",
        "markPrice":"224.025606310472291561","openInterest":"663.83","fundingRate":"0.00006758",
        "fundingTime":"1790582400000","nextFundingTime":"1790596800000"}"#;

    const NEVER_TRADED: &str = r#"{"contractId":"30000162","contractName":"EURUSDC","priceChange":"0",
        "priceChangePercent":"0","trades":"0","size":"0","value":"0","high":"0","low":"0","open":"0",
        "close":"0","highTime":"0","lowTime":"0","startTime":"1790497800000","endTime":"1790586000000",
        "lastPrice":"0","indexPrice":"1.1374765725","oraclePrice":"1.137526545483419604",
        "markPrice":"1.137526545483419604","openInterest":"0","fundingRate":"0.00000000",
        "fundingTime":"1790582400000","nextFundingTime":"1790596800000"}"#;

    fn ticker(raw: &str) -> RestTickerEdgex {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn ticker_and_mark_price_use_the_given_timestamp() {
        let raw = ticker(NVDA);
        assert_eq!(raw.nextFundingTime, 1_790_596_800_000);

        let last = raw.clone().into_ticker_data(9).unwrap();
        let mark = raw.into_mark_price_data(9).unwrap();

        assert_eq!(
            (last.inst.as_str(), last.price, last.timestamp),
            ("@30000020", 223.41, 9)
        );
        assert_eq!(last.inst_type, InstrumentType::Perpetual);
        assert_eq!(mark.inst, "@30000020");
        assert!((mark.mark_price - 224.025_606_31).abs() < 1e-9);
    }

    #[test]
    fn never_traded_contract_has_a_mark_but_no_last_price() {
        let mark = ticker(NEVER_TRADED).into_mark_price_data(1).unwrap();

        assert!(ticker(NEVER_TRADED).into_ticker_data(1).is_none());
        assert!((mark.mark_price - 1.137_526_545_48).abs() < 1e-9);
    }
}
