use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::utils_data::InstrumentInfo,
    api_general::{get_micros_timestamp, ts_to_micros},
    base_data::{InstrumentStatus, InstrumentType},
};

use crate::exchange::apex::api_utils::apex_symbol_to_cli;

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct RestSymbolsApex {
    pub contractConfig: ContractConfigApex,
}

/// Crypto perps and stock perps are listed apart; prediction contracts settle on an event.
#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct ContractConfigApex {
    #[serde(default)]
    pub perpetualContract: Vec<ContractApex>,
    #[serde(default)]
    pub stockContract: Vec<ContractApex>,
    #[serde(default)]
    pub predictionContract: Vec<ContractApex>,
}

impl ContractConfigApex {
    pub fn into_perps(self) -> impl Iterator<Item = ContractApex> {
        self.perpetualContract.into_iter().chain(self.stockContract)
    }
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct ContractApex {
    pub symbol: String,
    pub crossSymbolName: String,
    pub crossSymbolId: i64,
    pub l2PairId: String,
    pub baseTokenId: String,
    pub settleAssetId: String,
    pub tokenName: String,
    pub contractType: String,
    #[serde(default)]
    pub category: Option<String>,
    pub tag: String,
    pub tickSize: String,
    pub stepSize: String,
    pub minOrderSize: String,
    pub maxOrderSize: String,
    pub maxPositionSize: String,
    pub displayMaxLeverage: String,
    pub initialMarginRate: String,
    pub maintenanceMarginRate: String,
    pub fundingMaxRate: String,
    pub fundingMinRate: String,
    pub enableFundingSettlement: bool,
    pub enableTrade: bool,
    pub enableOpenPosition: bool,
    pub enableDisplay: bool,
    pub isPrelaunch: bool,
    #[serde(default)]
    pub disableOpenPositionTime: Option<u64>,
    #[serde(default)]
    pub pullOffTime: Option<u64>,
    #[serde(default)]
    pub deliveryTime: Option<u64>,
}

impl ContractApex {
    pub fn inst(&self) -> String {
        apex_symbol_to_cli(&self.crossSymbolName)
    }

    /// `stockContract` also lists commodities (XAU, CL) and index ETFs (SPY, QQQ) as `COMMODITY` / `INDEX`.
    pub fn is_stock(&self) -> bool {
        self.category.as_deref() == Some("STOCK")
    }

    pub fn state(&self) -> InstrumentStatus {
        apex_contract_status(self, get_micros_timestamp())
    }
}

fn apex_contract_status(d: &ContractApex, now_us: u64) -> InstrumentStatus {
    let scheduled_off = [d.disableOpenPositionTime, d.pullOffTime, d.deliveryTime]
        .into_iter()
        .flatten()
        .any(|ts| ts_to_micros(ts) > now_us);

    match (d.enableTrade, d.enableOpenPosition && d.enableDisplay) {
        (false, _) => InstrumentStatus::Closed,
        (true, false) => InstrumentStatus::Suspend,
        (true, true) if scheduled_off => InstrumentStatus::Delisting,
        (true, true) => InstrumentStatus::Live,
    }
}

impl From<ContractApex> for InstrumentInfo {
    fn from(d: ContractApex) -> Self {
        let lot_size = d.stepSize.parse().unwrap_or_default();
        let min_size = d.minOrderSize.parse().unwrap_or(lot_size);
        let max_size = d.maxOrderSize.parse().unwrap_or(f64::MAX);

        InstrumentInfo {
            inst: d.inst(),
            inst_code: Some(d.l2PairId.clone()),
            inst_type: InstrumentType::Perpetual,
            lot_size,
            tick_size: d.tickSize.parse().unwrap_or_default(),
            min_lmt_size: min_size,
            max_lmt_size: max_size,
            min_mkt_size: min_size,
            max_mkt_size: max_size,
            max_leverage: d.displayMaxLeverage.parse().ok(),
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

    const NVDA: &str = r#"{"baselinePositionValue":"50000","crossId":30002,"crossSymbolId":167,
        "crossSymbolName":"NVDAUSDT","digitMerge":"0.01,0.02,0.04,0.1,0.2","displayMaxLeverage":"50",
        "displayMinLeverage":"1","enableDisplay":true,"enableOpenPosition":true,"enableTrade":true,
        "fundingImpactMarginNotional":"50000","fundingInterestRate":"0.0003","initialMarginRate":"0.02000",
        "maintenanceMarginRate":"0.01","maxOrderSize":"600","maxPositionSize":"1400","minOrderSize":"0.01",
        "maxMarketPriceRange":"0.045","settleAssetId":"USDT","baseTokenId":"NVDA","tokenName":"NVIDIA",
        "stepSize":"0.01","symbol":"NVDA-USDT","symbolDisplayName":"NVDAUSDT","tickSize":"0.01",
        "tagIconUrl":"https://static-pro.apex.exchange/icon/7_24_H.svg","tag":"","riskTip":false,
        "enableFundingSettlement":true,"indexPriceDecimals":2,"fundingMaxRate":"0.005",
        "fundingMinRate":"-0.005","fundingMaxValue":"","l2PairId":"50162","settleTimeStamp":0,
        "isPrelaunch":false,"riskLimitConfig":{"positionSteps":["0","50000"],"imrSteps":["0.02000","0.02500"],
        "mmrSteps":["0.01","0.0125"]},"category":"STOCK","contractType":"STOCK_CONTRACT",
        "predictionContractType":"UNKNOWN_PREDICTION_CONTRACT_TYPE"}"#;

    const TON: &str = r#"{"crossSymbolId":13,"crossSymbolName":"TONUSDT","displayMaxLeverage":"50",
        "enableDisplay":false,"enableOpenPosition":false,"enableTrade":false,"initialMarginRate":"0.02000",
        "maintenanceMarginRate":"0.01","maxOrderSize":"12500","maxPositionSize":"25000","minOrderSize":"1",
        "settleAssetId":"USDT","baseTokenId":"TON","tokenName":"The Open Network","stepSize":"1",
        "symbol":"TON-USDT","tickSize":"0.0001","tag":"","enableFundingSettlement":false,
        "fundingMaxRate":"0.000305","fundingMinRate":"-0.000305","l2PairId":"50004","isPrelaunch":false,
        "pullOffTime":1782381600,"disableOpenPositionTime":1782381600,"category":"L1",
        "deliveryTime":1782385200000,"contractType":"UNKNOWN_CONTRACT_TYPE"}"#;

    const ETC: &str = r#"{"crossSymbolId":-1,"crossSymbolName":"ETCUSDT","displayMaxLeverage":"50",
        "enableDisplay":false,"enableOpenPosition":false,"enableTrade":false,"initialMarginRate":"0.02000",
        "maintenanceMarginRate":"0.01","maxOrderSize":"7500","maxPositionSize":"15000","minOrderSize":"0.1",
        "settleAssetId":"USDT","baseTokenId":"ETC","tokenName":"Ethereum Classic ","stepSize":"0.1",
        "symbol":"ETC-USDT","tickSize":"0.001","tag":"","enableFundingSettlement":false,
        "fundingMaxRate":"0.000305","fundingMinRate":"-0.000305","l2PairId":"50014","isPrelaunch":false,
        "category":"L1","contractType":"UNKNOWN_CONTRACT_TYPE"}"#;

    fn contract(raw: &str) -> ContractApex {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn stock_perp_maps_to_instrument_info() {
        let raw = contract(NVDA);
        assert!(raw.is_stock());
        assert_eq!(raw.inst(), "NVDA_USDT_PERP");

        let info = InstrumentInfo::from(raw);
        assert_eq!(info.inst, "NVDA_USDT_PERP");
        assert_eq!(info.inst_code.as_deref(), Some("50162"));
        assert_eq!(info.inst_type, InstrumentType::Perpetual);
        assert_eq!(info.tick_size, 0.01);
        assert_eq!(info.lot_size, 0.01);
        assert_eq!(info.min_lmt_size, 0.01);
        assert_eq!(info.max_lmt_size, 600.0);
        assert_eq!(info.max_mkt_size, 600.0);
        assert_eq!(info.max_leverage, Some(50));
        assert_eq!(info.min_notional, None);
        assert_eq!(info.state, InstrumentStatus::Live);
    }

    #[test]
    fn pulled_off_perp_is_closed() {
        let raw = contract(TON);

        assert!(!raw.is_stock());
        assert_eq!(raw.state(), InstrumentStatus::Closed);
        assert_eq!(InstrumentInfo::from(raw).min_lmt_size, 1.0);
    }

    #[test]
    fn retired_market_without_cross_symbol_id_parses() {
        let raw = contract(ETC);

        assert_eq!(raw.crossSymbolId, -1);
        assert_eq!(raw.inst(), "ETC_USDT_PERP");
        assert_eq!(raw.state(), InstrumentStatus::Closed);
    }

    #[test]
    fn statuses_follow_the_enable_flags_and_schedule() {
        let now = 1_790_585_000_000_000;
        let with = |from: &str, to: &str| contract(&NVDA.replace(from, to));

        let hidden = with(r#""enableDisplay":true"#, r#""enableDisplay":false"#);
        let reduce_only = with(
            r#""enableOpenPosition":true"#,
            r#""enableOpenPosition":false"#,
        );
        let scheduled = with(
            r#""isPrelaunch":false"#,
            r#""isPrelaunch":false,"pullOffTime":1790600000"#,
        );
        let past = with(
            r#""isPrelaunch":false"#,
            r#""isPrelaunch":false,"deliveryTime":1782385200000"#,
        );

        assert_eq!(
            apex_contract_status(&contract(NVDA), now),
            InstrumentStatus::Live
        );
        assert_eq!(
            apex_contract_status(&hidden, now),
            InstrumentStatus::Suspend
        );
        assert_eq!(
            apex_contract_status(&reduce_only, now),
            InstrumentStatus::Suspend
        );
        assert_eq!(
            apex_contract_status(&scheduled, now),
            InstrumentStatus::Delisting
        );
        assert_eq!(apex_contract_status(&past, now), InstrumentStatus::Live);
        assert_eq!(
            apex_contract_status(&contract(TON), now),
            InstrumentStatus::Closed
        );
    }

    #[test]
    fn missing_category_is_not_a_stock() {
        let raw = contract(&NVDA.replace(r#""category":"STOCK","#, ""));

        assert_eq!(raw.category, None);
        assert!(!raw.is_stock());
    }

    #[test]
    fn index_and_commodity_contracts_are_not_stocks() {
        for category in ["INDEX", "COMMODITY"] {
            let raw = contract(&NVDA.replace(r#""STOCK","#, &format!(r#""{category}","#)));
            assert!(!raw.is_stock(), "{category}");
        }
    }

    #[test]
    fn symbols_response_chains_crypto_and_stock_perps() {
        let raw = format!(
            r#"{{"contractConfig":{{"perpetualContract":[{TON}],"stockContract":[{NVDA}],
            "predictionContract":[],"stockCategorys":"All,STOCK,COMMODITY,INDEX"}}}}"#
        );
        let symbols: RestSymbolsApex = serde_json::from_str(&raw).unwrap();

        let insts: Vec<String> = symbols
            .contractConfig
            .into_perps()
            .map(|c| c.inst())
            .collect();
        assert_eq!(insts, vec!["TON_USDT_PERP", "NVDA_USDT_PERP"]);
    }
}
