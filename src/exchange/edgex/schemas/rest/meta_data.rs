use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::utils_data::InstrumentInfo,
    api_general::de_u64_from_string_or_number,
    base_data::{InstrumentStatus, InstrumentType},
};

use crate::exchange::edgex::api_utils::edgex_contract_to_cli;

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct RestMetaDataEdgex {
    pub coinList: Vec<CoinEdgex>,
    pub contractList: Vec<ContractEdgex>,
}

impl RestMetaDataEdgex {
    pub fn coin_name(&self, coin_id: &str) -> Option<&str> {
        self.coinList
            .iter()
            .find(|coin| coin.coinId == coin_id)
            .map(|coin| coin.coinName.as_str())
    }
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct CoinEdgex {
    pub coinId: String,
    pub coinName: String,
    pub stepSize: String,
    #[serde(default)]
    pub assetId: Option<String>,
    #[serde(default)]
    pub resolution: Option<String>,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct ContractEdgex {
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    pub contractId: u64,
    pub contractName: String,
    pub baseCoinId: String,
    pub quoteCoinId: String,
    pub tickSize: String,
    pub stepSize: String,
    pub minOrderSize: String,
    pub maxOrderSize: String,
    pub maxPositionSize: String,
    pub maxMarketPositionSize: String,
    #[serde(default)]
    pub riskTierList: Vec<RiskTierEdgex>,
    pub defaultTakerFeeRate: String,
    pub defaultMakerFeeRate: String,
    pub fundingRateIntervalMin: String,
    pub enableTrade: bool,
    pub enableDisplay: bool,
    pub enableOpenPosition: bool,
    #[serde(default)]
    pub isStock: bool,
    #[serde(default)]
    pub isFx: bool,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct RiskTierEdgex {
    pub tier: u32,
    pub positionValueUpperBound: String,
    pub maxLeverage: String,
    pub maintenanceMarginRate: String,
}

impl ContractEdgex {
    pub fn inst(&self) -> String {
        edgex_contract_to_cli(self.contractId)
    }

    /// Equities and ETFs (`NVDA`, `SPY`, `TENCENT`); FX, metals and energy are not.
    pub fn is_stock(&self) -> bool {
        self.isStock
    }

    pub fn state(&self) -> InstrumentStatus {
        match (self.enableTrade, self.enableOpenPosition) {
            (true, true) => InstrumentStatus::Live,
            _ => InstrumentStatus::Suspend,
        }
    }
}

impl From<ContractEdgex> for InstrumentInfo {
    fn from(d: ContractEdgex) -> Self {
        let lot_size = d.stepSize.parse().unwrap_or_default();
        let min_size = d.minOrderSize.parse().unwrap_or(lot_size);
        let max_size = d.maxOrderSize.parse().unwrap_or(f64::MAX);

        InstrumentInfo {
            inst: d.inst(),
            inst_code: Some(d.contractName.clone()),
            inst_type: InstrumentType::Perpetual,
            lot_size,
            tick_size: d.tickSize.parse().unwrap_or_default(),
            min_lmt_size: min_size,
            max_lmt_size: max_size,
            min_mkt_size: min_size,
            max_mkt_size: max_size,
            max_leverage: d
                .riskTierList
                .first()
                .and_then(|tier| tier.maxLeverage.parse().ok()),
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

    const NVDA: &str = r#"{"contractId":"30000020","contractName":"NVDAUSDC","baseCoinId":"1020",
        "quoteCoinId":"1000","tickSize":"0.01","stepSize":"0.01","minOrderSize":"0.2","maxOrderSize":"300",
        "maxOrderBuyPriceRatio":"0.2","minOrderSellPriceRatio":"0.2","maxPositionSize":"900",
        "maxMarketPositionSize":"65","riskTierList":[
            {"tier":1,"positionValueUpperBound":"200000","maxLeverage":"20","maintenanceMarginRate":"0.025",
             "risk":"107374182","upperBound":"858993459200000000000"},
            {"tier":2,"positionValueUpperBound":"300000","maxLeverage":"15","maintenanceMarginRate":"0.0333",
             "risk":"143022410","upperBound":"1288490188800000000000"}],
        "defaultTakerFeeRate":"0.00045","defaultMakerFeeRate":"0.0004","defaultLeverage":"10",
        "liquidateFeeRate":"0.01","tpslSpreadProtectionThreshold":"0.03","enableTrade":true,
        "enableDisplay":true,"enableOpenPosition":true,"fundingInterestRate":"0.0003",
        "fundingImpactMarginNotional":"10","fundingMaxRate":"0.004","fundingMinRate":"-0.004",
        "fundingRateIntervalMin":"240","fundingQuoteImpactMarginNotional":"10",
        "displayDigitMerge":"0.01,0.02,0.05,0.1","displayMaxLeverage":"20","displayMinLeverage":"1",
        "displayNewIcon":false,"displayHotIcon":false,"matchServerName":"edgex-match-server",
        "syntheticAssetId":"0x4e5644415553444300000000000000","resolution":"1000000000",
        "oraclePriceQuorum":"1","oraclePriceSignedAssetIds":["NVDAUSD"],
        "oraclePriceSigners":["0xb50cbeba205ddb685db2497eaea07f31a18a53ad"],
        "openSizeReduceRatio":"0.99","isStock":true,"isFx":false}"#;

    fn contract(raw: &str) -> ContractEdgex {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn stock_perp_maps_to_instrument_info() {
        let raw = contract(NVDA);
        assert!(raw.is_stock());
        assert_eq!(raw.contractId, 30000020);
        assert_eq!(raw.riskTierList.len(), 2);

        let info = InstrumentInfo::from(raw);
        assert_eq!(info.inst, "@30000020");
        assert_eq!(info.inst_code.as_deref(), Some("NVDAUSDC"));
        assert_eq!(info.inst_type, InstrumentType::Perpetual);
        assert_eq!(info.tick_size, 0.01);
        assert_eq!(info.lot_size, 0.01);
        assert_eq!(info.min_lmt_size, 0.2);
        assert_eq!(info.max_lmt_size, 300.0);
        assert_eq!(info.min_mkt_size, 0.2);
        assert_eq!(info.max_leverage, Some(20));
        assert_eq!(info.min_notional, None);
        assert_eq!(info.state, InstrumentStatus::Live);
    }

    #[test]
    fn closed_or_reduce_only_contracts_are_suspended() {
        let closed = NVDA.replace(r#""enableTrade":true"#, r#""enableTrade":false"#);
        let reduce_only = NVDA.replace(
            r#""enableOpenPosition":true"#,
            r#""enableOpenPosition":false"#,
        );
        let hidden = NVDA.replace(r#""enableDisplay":true"#, r#""enableDisplay":false"#);

        assert_eq!(contract(&closed).state(), InstrumentStatus::Suspend);
        assert_eq!(contract(&reduce_only).state(), InstrumentStatus::Suspend);
        assert_eq!(contract(&hidden).state(), InstrumentStatus::Live);
    }

    #[test]
    fn contract_without_risk_tiers_has_no_leverage() {
        let start = NVDA.find(r#""riskTierList""#).unwrap();
        let end = NVDA.find(r#""defaultTakerFeeRate""#).unwrap();
        let raw = format!("{}{}", &NVDA[..start], &NVDA[end..]);

        let info = InstrumentInfo::from(contract(&raw));
        assert_eq!(info.max_leverage, None);
    }

    #[test]
    fn metadata_names_coins_and_tolerates_null_asset_ids() {
        let raw = format!(
            r#"{{"global":{{"appName":"edgeX","appEnv":"mainnet"}},"coinList":[
            {{"coinId":"1000","coinName":"USDC","stepSize":"0.000001","showStepSize":"0.0001",
              "iconUrl":"https://static.edgex.exchange/icons/coin/USDC.png",
              "assetId":"0x2ce625e94458d39dd0bf3b45a843544dd4a14b8169045a3a3d15aa564b936c5","resolution":"0xf4240"}},
            {{"coinId":"1020","coinName":"NVDA","stepSize":"0.01","showStepSize":"0.01",
              "iconUrl":"https://static.edgex.exchange/icons/coin/NVDA.png","assetId":null,"resolution":null}}],
            "contractList":[{NVDA}],"multiChain":{{"chainList":[]}}}}"#
        );
        let meta: RestMetaDataEdgex = serde_json::from_str(&raw).unwrap();

        assert_eq!(meta.coinList[1].assetId, None);
        let nvda = &meta.contractList[0];
        assert_eq!(meta.coin_name(&nvda.baseCoinId), Some("NVDA"));
        assert_eq!(meta.coin_name(&nvda.quoteCoinId), Some("USDC"));
        assert_eq!(meta.coin_name("9999"), None);
    }
}
