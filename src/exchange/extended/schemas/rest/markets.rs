use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::{
        price_data::{MarkPriceData, TickerData},
        utils_data::InstrumentInfo,
    },
    base_data::{InstrumentStatus, InstrumentType},
};

use crate::exchange::extended::api_utils::{extended_market_to_cli, extended_spot_to_cli};

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct MarketExtended {
    pub name: String,
    pub uiName: String,
    #[serde(rename = "type")]
    pub marketType: String,
    pub category: String,
    pub subCategory: String,
    pub assetName: String,
    pub collateralAssetName: String,
    pub active: bool,
    pub status: String,
    pub isRfq: bool,
    pub isOffHours: bool,
    pub tradingHours: String,
    pub marketStats: MarketStatsExtended,
    pub tradingConfig: TradingConfigExtended,
}

/// Prices the venue does not have, such as spot mark prices or an untraded market's last price, are `"0"`.
#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct MarketStatsExtended {
    pub dailyVolume: String,
    pub lastPrice: String,
    pub askPrice: String,
    pub bidPrice: String,
    pub markPrice: String,
    pub indexPrice: String,
    pub fundingRate: String,
    /// Despite the name, the next funding time in milliseconds.
    pub nextFundingRate: u64,
    pub openInterest: String,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct TradingConfigExtended {
    pub minOrderSize: String,
    pub minOrderSizeChange: String,
    pub minPriceChange: String,
    pub maxMarketOrderValue: String,
    pub maxLimitOrderValue: String,
    pub maxPositionValue: String,
    pub maxLeverage: String,
}

impl MarketExtended {
    pub fn inst(&self) -> String {
        match self.inst_type() {
            InstrumentType::Spot => extended_spot_to_cli(&self.name),
            _ => extended_market_to_cli(&self.name),
        }
    }

    /// US equities are `NVDA_24_5-USD` style 24/5 markets or plain `SHOP-USD`; `uiName` drops the `_24_5`.
    pub fn is_equity(&self) -> bool {
        self.subCategory == "Equity"
    }

    pub fn inst_type(&self) -> InstrumentType {
        match self.marketType.as_str() {
            "PERPETUAL" => InstrumentType::Perpetual,
            "SPOT" => InstrumentType::Spot,
            _ => InstrumentType::Unknown,
        }
    }

    pub fn state(&self) -> InstrumentStatus {
        match self.status.as_str() {
            "ACTIVE" if self.isOffHours => InstrumentStatus::Suspend,
            "ACTIVE" => InstrumentStatus::Live,
            "REDUCE_ONLY" | "DISABLED" => InstrumentStatus::Suspend,
            "PRELISTED" => InstrumentStatus::PreOpen,
            "DELISTED" => InstrumentStatus::Closed,
            _ => InstrumentStatus::Unknown,
        }
    }

    pub fn into_ticker_data(self, timestamp: u64) -> Option<TickerData> {
        let price = positive_price(&self.marketStats.lastPrice)?;

        Some(TickerData {
            timestamp,
            inst: self.inst(),
            inst_type: self.inst_type(),
            price,
        })
    }

    pub fn into_mark_price_data(self, timestamp: u64) -> Option<MarkPriceData> {
        let mark_price = positive_price(&self.marketStats.markPrice)?;

        Some(MarkPriceData {
            timestamp,
            inst: self.inst(),
            inst_type: self.inst_type(),
            mark_price,
        })
    }
}

fn positive_price(price: &str) -> Option<f64> {
    price.parse().ok().filter(|price: &f64| *price > 0.0)
}

impl From<MarketExtended> for InstrumentInfo {
    fn from(d: MarketExtended) -> Self {
        let config = &d.tradingConfig;
        let lot_size = config.minOrderSizeChange.parse().unwrap_or_default();
        let min_size = config.minOrderSize.parse().unwrap_or(lot_size);
        let max_leverage: f64 = config.maxLeverage.parse().unwrap_or_default();

        InstrumentInfo {
            inst: d.inst(),
            inst_code: None,
            inst_type: d.inst_type(),
            lot_size,
            tick_size: config.minPriceChange.parse().unwrap_or_default(),
            min_lmt_size: min_size,
            max_lmt_size: f64::MAX,
            min_mkt_size: min_size,
            max_mkt_size: f64::MAX,
            max_leverage: (max_leverage > 0.0).then(|| max_leverage.round() as u32),
            min_notional: None,
            contract_value: None,
            contract_multiplier: None,
            state: d.state(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NVDA: &str = r#"{"name":"NVDA_24_5-USD","type":"PERPETUAL","uiName":"NVDA-USD","category":"RWA",
        "subCategory":"Equity","assetName":"NVDA_24_5","assetPrecision":2,"collateralAssetName":"USD",
        "collateralAssetPrecision":6,"description":"NVIDIA","active":true,"isRfq":false,"isOffHours":false,
        "status":"ACTIVE","tradingHours":"CONTINUOUS","marketStats":{"dailyVolume":"311280.873200",
        "dailyVolumeBase":"1384.68","dailyPriceChange":"-1.10","dailyPriceChangePercentage":"-0.0049",
        "dailyLow":"222.95","dailyHigh":"225.56","lastPrice":"224.25","askPrice":"223.9","bidPrice":"223.75",
        "markPrice":"223.879999999999","indexPrice":"223.879999999999","fundingRate":"0.000004",
        "nextFundingRate":1790589600000,"openInterest":"576958.306604","openInterestBase":"2572.42",
        "deleverageLevels":{"shortPositions":[{"level":1,"rankingLowerBound":"-46.5362"}],
        "longPositions":[{"level":1,"rankingLowerBound":"-78.8277"}]}},"tradingConfig":{"minOrderSize":"0.1",
        "minOrderSizeChange":"0.01","minPriceChange":"0.01","maxMarketOrderValue":"150000",
        "maxLimitOrderValue":"750000","maxPositionValue":"1000000","maxLeverage":"10.00",
        "hourlyFundingRateCap":"1","openInterestLimit":"0","maxNumOrders":"200","limitPriceCap":"0.15",
        "limitPriceFloor":"0.15","riskFactorConfig":[{"upperBound":"500000","riskFactor":"0.1",
        "isAvailableForUsers":true}],"uPnlWithdrawable":false,"postWithdrawalMarginFactor":"0"},
        "l2Config":{"type":"STARKX","collateralId":"0x1","syntheticId":"0x4e5644415f32345f35000000000000",
        "syntheticResolution":1000,"collateralResolution":1000000},"visibleOnUi":true,
        "referenceMarket":"us_equity","createdAt":1770891119064}"#;

    const SPOT: &str = r#"{"name":"BTCSPOT-USD","type":"SPOT","uiName":"wBTC/USDC","category":"Crypto",
        "subCategory":"L1","assetName":"BTCSPOT","assetPrecision":8,"collateralAssetName":"USD",
        "collateralAssetPrecision":6,"description":"Wrapped Bitcoin","active":true,"isRfq":false,
        "isOffHours":false,"status":"ACTIVE","tradingHours":"CONTINUOUS","marketStats":{
        "dailyVolume":"0.000000","dailyVolumeBase":"0.00000000","dailyPriceChange":"0",
        "dailyPriceChangePercentage":"0","dailyLow":"0","dailyHigh":"0","lastPrice":"85613",
        "askPrice":"82697","bidPrice":"82601","markPrice":"0","indexPrice":"82658.570997954302",
        "fundingRate":"0","nextFundingRate":0,"openInterest":"0","openInterestBase":"0",
        "deleverageLevels":{"shortPositions":[],"longPositions":[]}},"tradingConfig":{
        "minOrderSize":"0.0001","minOrderSizeChange":"0.00001","minPriceChange":"1",
        "maxMarketOrderValue":"250000","maxLimitOrderValue":"1250000","maxPositionValue":"2500000",
        "maxLeverage":"1","hourlyFundingRateCap":"0.25","openInterestLimit":"0","maxNumOrders":"200",
        "limitPriceCap":"0.15","limitPriceFloor":"0.15","riskFactorConfig":[{"upperBound":"2500000",
        "riskFactor":"1","isAvailableForUsers":true}],"uPnlWithdrawable":false,
        "postWithdrawalMarginFactor":"0"},"l2Config":{"type":"STARKX","collateralId":"0x1",
        "syntheticId":"0x03fe2b97c1fd336e750087d68b9b867997fd64a2661ff3ca5a7c771641e8e7ac",
        "syntheticResolution":100000000,"collateralResolution":1000000},"visibleOnUi":true,
        "referenceMarket":"null","createdAt":1778084398595}"#;

    const DELISTED: &str = r#"{"name":"PLACE_JPY-USD_1-USD","type":"PERPETUAL","uiName":"PLACE_JPY-USD_1-USD",
        "category":"RWA","subCategory":"TradFi","assetName":"PLACE_JPY_1","assetPrecision":2,
        "collateralAssetName":"USD","collateralAssetPrecision":6,"description":"PLACE_JPY_1","active":false,
        "isRfq":false,"isOffHours":false,"status":"DELISTED","tradingHours":"CONTINUOUS","marketStats":{
        "dailyVolume":"0","dailyVolumeBase":"0","dailyPriceChange":"0","dailyPriceChangePercentage":"0",
        "dailyLow":"0","dailyHigh":"0","lastPrice":"0","askPrice":"0","bidPrice":"0","markPrice":"0",
        "indexPrice":"0","fundingRate":"0.000013","nextFundingRate":1790589600000,"openInterest":"0",
        "openInterestBase":"0","deleverageLevels":{"shortPositions":[],"longPositions":[]}},
        "tradingConfig":{"minOrderSize":"0.1","minOrderSizeChange":"0.01","minPriceChange":"0.001",
        "maxMarketOrderValue":"250000","maxLimitOrderValue":"1250000","maxPositionValue":"2500000",
        "maxLeverage":"20.00","hourlyFundingRateCap":"1","openInterestLimit":"0","maxNumOrders":"200",
        "limitPriceCap":"0.1","limitPriceFloor":"0.1","riskFactorConfig":[{"upperBound":"500000",
        "riskFactor":"0.05","isAvailableForUsers":true}],"uPnlWithdrawable":false,
        "postWithdrawalMarginFactor":"0"},"l2Config":{"type":"STARKX","collateralId":"0x1",
        "syntheticId":"0x5553442d4a50592d38000000000000","syntheticResolution":1000,
        "collateralResolution":1000000},"visibleOnUi":false,"referenceMarket":"null","createdAt":1773304042484}"#;

    fn market(raw: &str) -> MarketExtended {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn equity_perp_maps_to_instrument_info() {
        let raw = market(NVDA);
        assert!(raw.is_equity());
        assert_eq!(raw.uiName, "NVDA-USD");
        assert_eq!(raw.marketStats.nextFundingRate, 1_790_589_600_000);

        let info = InstrumentInfo::from(raw);
        assert_eq!(info.inst, "NVDA_24_5_USD_PERP");
        assert_eq!(info.inst_code, None);
        assert_eq!(info.inst_type, InstrumentType::Perpetual);
        assert_eq!(info.tick_size, 0.01);
        assert_eq!(info.lot_size, 0.01);
        assert_eq!(info.min_lmt_size, 0.1);
        assert_eq!(info.min_mkt_size, 0.1);
        assert_eq!(info.max_lmt_size, f64::MAX);
        assert_eq!(info.max_leverage, Some(10));
        assert_eq!(info.min_notional, None);
        assert_eq!(info.state, InstrumentStatus::Live);
    }

    #[test]
    fn spot_is_named_without_the_perp_suffix() {
        let raw = market(SPOT);
        assert!(!raw.is_equity());
        assert_eq!(raw.inst(), "BTCSPOT_USD");
        assert_eq!(raw.inst_type(), InstrumentType::Spot);
        assert!(raw.clone().into_mark_price_data(1).is_none());
        assert_eq!(raw.clone().into_ticker_data(1).unwrap().price, 85613.0);
        assert_eq!(InstrumentInfo::from(raw).max_leverage, Some(1));
    }

    #[test]
    fn statuses_map_to_instrument_states() {
        let with = |status: &str, off_hours: bool| {
            let raw = NVDA
                .replace(r#""status":"ACTIVE""#, &format!(r#""status":"{status}""#))
                .replace(
                    r#""isOffHours":false"#,
                    &format!(r#""isOffHours":{off_hours}"#),
                );
            market(&raw).state()
        };

        assert_eq!(with("ACTIVE", false), InstrumentStatus::Live);
        assert_eq!(with("ACTIVE", true), InstrumentStatus::Suspend);
        assert_eq!(with("REDUCE_ONLY", false), InstrumentStatus::Suspend);
        assert_eq!(with("DISABLED", false), InstrumentStatus::Suspend);
        assert_eq!(with("PRELISTED", false), InstrumentStatus::PreOpen);
        assert_eq!(with("DELISTED", false), InstrumentStatus::Closed);
        assert_eq!(with("NEW", false), InstrumentStatus::Unknown);
    }

    #[test]
    fn delisted_market_has_no_prices_and_keeps_its_venue_name() {
        let raw = market(DELISTED);

        assert_eq!(raw.inst(), "PLACE_JPY-USD_1_USD_PERP");
        assert_eq!(raw.state(), InstrumentStatus::Closed);
        assert!(raw.clone().into_ticker_data(1).is_none());
        assert!(raw.into_mark_price_data(1).is_none());
    }

    #[test]
    fn ticker_and_mark_price_come_from_market_stats() {
        let ticker = market(NVDA).into_ticker_data(9).unwrap();
        let mark = market(NVDA).into_mark_price_data(9).unwrap();

        assert_eq!(
            (ticker.inst.as_str(), ticker.price, ticker.timestamp),
            ("NVDA_24_5_USD_PERP", 224.25, 9)
        );
        assert_eq!(mark.mark_price, 223.879999999999);
        assert_eq!(mark.inst_type, InstrumentType::Perpetual);
    }
}
