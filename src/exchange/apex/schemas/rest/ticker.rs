use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::price_data::{MarkPriceData, TickerData},
    base_data::InstrumentType,
};

use crate::exchange::apex::api_utils::apex_symbol_to_cli;

/// One entry of `all-ticker-info`; it carries no exchange time.
#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct RestTickerApex {
    pub symbol: String,
    pub lastPrice: String,
    pub markPrice: String,
    pub indexPrice: String,
    pub fundingRate: String,
    pub predictedFundingRate: String,
    pub nextFundingTime: String,
    pub openInterest: String,
    pub volume24h: String,
    pub turnover24h: String,
    pub price24hPcnt: String,
}

impl RestTickerApex {
    pub fn inst(&self) -> String {
        apex_symbol_to_cli(&self.symbol)
    }

    pub fn into_ticker_data(self, timestamp: u64) -> Option<TickerData> {
        let price = self.lastPrice.parse().ok()?;

        Some(TickerData {
            timestamp,
            inst: self.inst(),
            inst_type: InstrumentType::Perpetual,
            price,
        })
    }

    pub fn into_mark_price_data(self, timestamp: u64) -> Option<MarkPriceData> {
        let mark_price = self.markPrice.parse().ok()?;

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

    const NVDA: &str = r#"{"iconUrl":"https://static-pro.apex.exchange/icon/NVDA.png","fundingRate":"0.0000125",
        "highPrice24h":"225.73","indexPrice":"223.32","lastPrice":"223.21","lowPrice24h":"222.95",
        "nextFundingTime":"2026-09-28T09:00:00Z","openInterest":"69.28000000000002","oraclePrice":"",
        "markPrice":"223.31","predictedFundingRate":"0.0000125","price24hPcnt":"-0.0080878105141537",
        "symbol":"NVDAUSDT","tradeCount":"","turnover24h":"37467.1288","volume24h":"166.67"}"#;

    fn ticker(raw: &str) -> RestTickerApex {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn ticker_and_mark_price_use_the_given_timestamp() {
        let last = ticker(NVDA).into_ticker_data(7).unwrap();
        let mark = ticker(NVDA).into_mark_price_data(7).unwrap();

        assert_eq!(
            (last.inst.as_str(), last.price, last.timestamp),
            ("NVDA_USDT_PERP", 223.21, 7)
        );
        assert_eq!(last.inst_type, InstrumentType::Perpetual);
        assert_eq!(
            (mark.inst.as_str(), mark.mark_price),
            ("NVDA_USDT_PERP", 223.31)
        );
    }

    #[test]
    fn empty_prices_are_skipped() {
        let raw = NVDA
            .replace(r#""lastPrice":"223.21""#, r#""lastPrice":"""#)
            .replace(r#""markPrice":"223.31""#, r#""markPrice":"""#);

        assert!(ticker(&raw).into_ticker_data(1).is_none());
        assert!(ticker(&raw).into_mark_price_data(1).is_none());
    }
}
