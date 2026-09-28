use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::{
        price_data::{MarkPriceData, TickerData},
        utils_data::InstrumentInfo,
    },
    base_data::{InstrumentStatus, InstrumentType},
};

use crate::exchange::arcus::api_utils::arcus_market_to_cli;

#[derive(Clone, Debug, Deserialize)]
pub struct RestMarketsArcus {
    pub markets: Vec<MarketArcus>,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct MarketArcus {
    pub marketDisplayName: String,
    pub marketId: u16,
    pub status: String,
    #[serde(rename = "type")]
    pub marketType: String,
    pub category: String,
    pub baseAsset: String,
    pub quoteAsset: String,
    pub tickSize: String,
    #[serde(default)]
    pub tickTiers: Vec<TickTierArcus>,
    pub stepSize: String,
    pub minOrderNotional: String,
    pub minOrderSize: String,
    pub maxOrderSize: String,
    #[serde(default)]
    pub oraclePrice: Option<String>,
    #[serde(default)]
    pub markPrice: Option<String>,
    #[serde(default)]
    pub lastTradePrice: Option<String>,
    #[serde(default)]
    pub fundingRate: Option<String>,
    #[serde(default)]
    pub nextFundingAt: Option<u64>,
    pub initialMarginFraction: String,
    pub maintenanceMarginFraction: String,
    #[serde(default)]
    pub offHoursInitialMarginFraction: Option<String>,
    #[serde(default)]
    pub regularTradingHours: Option<TradingHoursArcus>,
    #[serde(default)]
    pub isOutsideRth: Option<bool>,
    #[serde(default)]
    pub currentSettlementPrice: Option<String>,
    #[serde(default)]
    pub upperTradingBound: Option<String>,
    #[serde(default)]
    pub lowerTradingBound: Option<String>,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct TickTierArcus {
    #[serde(default)]
    pub upToPrice: Option<String>,
    pub tick: String,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct TradingHoursArcus {
    pub startSecondsOfDay: u32,
    pub endSecondsOfDay: u32,
    pub timezone: String,
    pub isOvernight: bool,
}

impl MarketArcus {
    pub fn inst(&self) -> String {
        arcus_market_to_cli(&self.marketDisplayName)
    }

    pub fn is_equity(&self) -> bool {
        self.category == "EQUITIES"
    }

    pub fn inst_type(&self) -> InstrumentType {
        match self.marketType.as_str() {
            "PERPETUAL" => InstrumentType::Perpetual,
            _ => InstrumentType::Unknown,
        }
    }

    pub fn state(&self) -> InstrumentStatus {
        match self.status.as_str() {
            "ONLINE" => InstrumentStatus::Live,
            "OFFLINE" => InstrumentStatus::Suspend,
            _ => InstrumentStatus::Unknown,
        }
    }

    pub fn into_ticker_data(self, timestamp: u64) -> Option<TickerData> {
        let price = self.lastTradePrice.as_deref()?.parse().ok()?;

        Some(TickerData {
            timestamp,
            inst: self.inst(),
            inst_type: self.inst_type(),
            price,
        })
    }

    pub fn into_mark_price_data(self, timestamp: u64) -> Option<MarkPriceData> {
        let mark_price = self.markPrice.as_deref()?.parse().ok()?;

        Some(MarkPriceData {
            timestamp,
            inst: self.inst(),
            inst_type: self.inst_type(),
            mark_price,
        })
    }
}

impl From<MarketArcus> for InstrumentInfo {
    fn from(d: MarketArcus) -> Self {
        let lot_size = d.stepSize.parse().unwrap_or_default();
        let min_size = d.minOrderSize.parse().unwrap_or(lot_size);
        let max_size = d.maxOrderSize.parse().unwrap_or(f64::MAX);
        let min_notional: f64 = d.minOrderNotional.parse().unwrap_or_default();
        let initial_margin: f64 = d.initialMarginFraction.parse().unwrap_or_default();

        InstrumentInfo {
            inst: d.inst(),
            inst_code: Some(d.marketId.to_string()),
            inst_type: d.inst_type(),
            lot_size,
            tick_size: d.tickSize.parse().unwrap_or_default(),
            min_lmt_size: min_size,
            max_lmt_size: max_size,
            min_mkt_size: min_size,
            max_mkt_size: max_size,
            max_leverage: (initial_margin > 0.0).then(|| (1.0 / initial_margin).round() as u32),
            min_notional: (min_notional > 0.0).then_some(min_notional),
            contract_value: None,
            contract_multiplier: None,
            state: d.state(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NVDA: &str = r#"{"marketDisplayName":"NVDA-USD","fullAssetName":"NVIDIA","marketId":28,
        "status":"ONLINE","baseAsset":"NVDA","quoteAsset":"USD","tickSize":"0.01","stepSize":"0.0000001",
        "tickTiers":[{"upToPrice":"5000","tick":"0.01"},{"upToPrice":"10000","tick":"0.02"},{"tick":"0.5"}],
        "minOrderNotional":"5","minOrderSize":"0.01","maxOrderSize":"100000","oraclePrice":"223.48",
        "markPrice":"223.44","lastTradePrice":"223.43","fundingRate":"0.000005069444444444",
        "nextFundingRate":"0.00000474537037037","nextFundingAt":1790578800,
        "openInterestCapNotional":"500000","initialMarginFraction":"0.05",
        "maintenanceMarginFraction":"0.033334","offHoursInitialMarginFraction":"0.075",
        "regularTradingHours":{"startSecondsOfDay":14400,"endSecondsOfDay":72000,
            "timezone":"America/New_York","isOvernight":false},
        "isOutsideRth":true,"currentSettlementPrice":"224.96","upperTradingBound":"230.58",
        "lowerTradingBound":"219.34","isUpperInExpansionZone":false,"upperZoneEnteredAt":null,
        "type":"PERPETUAL","category":"EQUITIES","addedTimestamp":1747310340,
        "assetResolution":"10000000000","pythId":"1314"}"#;

    fn market(raw: &str) -> MarketArcus {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn equity_perp_maps_to_instrument_info() {
        let raw = market(NVDA);
        assert!(raw.is_equity());
        assert_eq!(raw.isOutsideRth, Some(true));
        assert_eq!(raw.tickTiers.len(), 3);
        assert_eq!(
            raw.regularTradingHours.as_ref().unwrap().startSecondsOfDay,
            14400
        );

        let info = InstrumentInfo::from(raw);
        assert_eq!(info.inst, "NVDA_USD_PERP");
        assert_eq!(info.inst_code.as_deref(), Some("28"));
        assert_eq!(info.inst_type, InstrumentType::Perpetual);
        assert_eq!(info.tick_size, 0.01);
        assert_eq!(info.lot_size, 0.0000001);
        assert_eq!(info.min_lmt_size, 0.01);
        assert_eq!(info.max_lmt_size, 100000.0);
        assert_eq!(info.min_notional, Some(5.0));
        assert_eq!(info.max_leverage, Some(20));
        assert_eq!(info.state, InstrumentStatus::Live);
    }

    #[test]
    fn null_outside_rth_parses() {
        let raw = market(&NVDA.replace(r#""isOutsideRth":true"#, r#""isOutsideRth":null"#));

        assert_eq!(raw.isOutsideRth, None);
    }

    #[test]
    fn offline_market_is_suspended() {
        let raw = market(&NVDA.replace(r#""status":"ONLINE""#, r#""status":"OFFLINE""#));

        assert_eq!(raw.state(), InstrumentStatus::Suspend);
    }

    #[test]
    fn crypto_market_without_rth_fields_parses() {
        let raw = r#"{"marketDisplayName":"BTC-USD","marketId":1,"status":"ONLINE","baseAsset":"BTC",
            "quoteAsset":"USD","tickSize":"0.1","stepSize":"0.00000001","minOrderNotional":"5",
            "minOrderSize":"0.0001","maxOrderSize":"10000","initialMarginFraction":"0.025",
            "maintenanceMarginFraction":"0.0125","type":"PERPETUAL","category":"CRYPTO"}"#;

        let m = market(raw);
        assert!(!m.is_equity());
        assert!(m.clone().into_mark_price_data(1).is_none());
        assert_eq!(InstrumentInfo::from(m).max_leverage, Some(40));
    }

    #[test]
    fn ticker_and_mark_price_come_from_the_market() {
        let ticker = market(NVDA).into_ticker_data(9).unwrap();
        let mark = market(NVDA).into_mark_price_data(9).unwrap();

        assert_eq!(
            (ticker.inst.as_str(), ticker.price, ticker.timestamp),
            ("NVDA_USD_PERP", 223.43, 9)
        );
        assert_eq!(mark.mark_price, 223.44);
    }
}
