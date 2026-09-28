use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::utils_data::InstrumentInfo,
    base_data::{InstrumentStatus, InstrumentType},
};

use crate::exchange::pacifica::api_utils::pacifica_symbol_to_cli;

/// One `/info` market. It carries no status or asset category: equities, FX and
/// commodities are listed like crypto perps.
#[derive(Clone, Debug, Deserialize)]
pub struct MarketInfoPacifica {
    pub symbol: String,
    pub base_asset: String,
    pub instrument_type: String,
    pub tick_size: String,
    pub min_tick: String,
    pub max_tick: String,
    pub lot_size: String,
    pub max_leverage: u32,
    pub isolated_only: bool,
    /// USD notional.
    pub min_order_size: String,
    /// USD notional.
    pub max_order_size: String,
    pub funding_rate: String,
    pub next_funding_rate: String,
    pub created_at: u64,
    #[serde(default)]
    pub execution_modes: Vec<String>,
}

impl MarketInfoPacifica {
    pub fn inst(&self) -> String {
        pacifica_symbol_to_cli(&self.symbol)
    }

    pub fn inst_type(&self) -> InstrumentType {
        match self.instrument_type.as_str() {
            "perpetual" => InstrumentType::Perpetual,
            "spot" => InstrumentType::Spot,
            _ => InstrumentType::Unknown,
        }
    }
}

impl From<MarketInfoPacifica> for InstrumentInfo {
    fn from(d: MarketInfoPacifica) -> Self {
        let lot_size = d.lot_size.parse().unwrap_or_default();
        let min_notional: f64 = d.min_order_size.parse().unwrap_or_default();

        InstrumentInfo {
            inst: d.inst(),
            inst_code: Some(d.symbol.clone()),
            inst_type: d.inst_type(),
            lot_size,
            tick_size: d.tick_size.parse().unwrap_or_default(),
            min_lmt_size: lot_size,
            max_lmt_size: f64::MAX,
            min_mkt_size: lot_size,
            max_mkt_size: f64::MAX,
            max_leverage: (d.max_leverage > 0).then_some(d.max_leverage),
            min_notional: (min_notional > 0.0).then_some(min_notional),
            contract_value: None,
            contract_multiplier: None,
            // `/info` has no status field, so every listed market counts as live.
            state: InstrumentStatus::Live,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NVDA: &str = r#"{"symbol":"NVDA","tick_size":"0.01","min_tick":"0","max_tick":"10000000",
        "lot_size":"0.001","max_leverage":10,"isolated_only":false,"min_order_size":"10",
        "max_order_size":"1000000","funding_rate":"0.0000125","next_funding_rate":"0.0000125",
        "created_at":1766907878723,"instrument_type":"perpetual","base_asset":"NVDA",
        "execution_modes":["orderbook","rfq"]}"#;

    const KBONK: &str = r#"{"symbol":"kBONK","tick_size":"0.000001","min_tick":"0","max_tick":"10000000",
        "lot_size":"1","max_leverage":10,"isolated_only":false,"min_order_size":"10",
        "max_order_size":"1000000","funding_rate":"0.0000125","next_funding_rate":"0.0000125",
        "created_at":1754008594378,"instrument_type":"perpetual","base_asset":"kBONK",
        "execution_modes":["orderbook"]}"#;

    const SPOT: &str = r#"{"symbol":"SOL-USDC","tick_size":"0.01","min_tick":"0","max_tick":"1000000",
        "lot_size":"0.001","max_leverage":1,"isolated_only":false,"min_order_size":"10",
        "max_order_size":"1000000","funding_rate":"0","next_funding_rate":"0",
        "created_at":1776615970246,"instrument_type":"spot","base_asset":"SOL",
        "execution_modes":["orderbook"]}"#;

    fn market(raw: &str) -> MarketInfoPacifica {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn stock_perp_maps_to_instrument_info() {
        let raw = market(NVDA);
        assert_eq!(raw.execution_modes, vec!["orderbook", "rfq"]);

        let info = InstrumentInfo::from(raw);
        assert_eq!(info.inst, "NVDA_USDC_PERP");
        assert_eq!(info.inst_code.as_deref(), Some("NVDA"));
        assert_eq!(info.inst_type, InstrumentType::Perpetual);
        assert_eq!(info.tick_size, 0.01);
        assert_eq!(info.lot_size, 0.001);
        assert_eq!(info.min_lmt_size, 0.001);
        assert_eq!(info.max_lmt_size, f64::MAX);
        assert_eq!(info.min_notional, Some(10.0));
        assert_eq!(info.max_leverage, Some(10));
        assert_eq!(info.state, InstrumentStatus::Live);
    }

    #[test]
    fn kilo_prefix_keeps_the_venue_case() {
        let info = InstrumentInfo::from(market(KBONK));

        assert_eq!(info.inst, "kBONK_USDC_PERP");
        assert_eq!(info.inst_code.as_deref(), Some("kBONK"));
        assert_eq!(info.tick_size, 0.000001);
    }

    #[test]
    fn spot_market_is_a_spot_pair() {
        let raw = market(SPOT);
        assert_eq!(raw.base_asset, "SOL");

        let info = InstrumentInfo::from(raw);
        assert_eq!(info.inst, "SOL_USDC");
        assert_eq!(info.inst_type, InstrumentType::Spot);
        assert_eq!(info.max_leverage, Some(1));
    }

    #[test]
    fn unknown_instrument_type_is_unknown() {
        let raw = market(&NVDA.replace(r#""perpetual""#, r#""dated_future""#));

        assert_eq!(raw.inst_type(), InstrumentType::Unknown);
    }

    #[test]
    fn missing_execution_modes_parses() {
        let raw = market(&NVDA.replace("execution_modes", "other_modes"));

        assert!(raw.execution_modes.is_empty());
    }
}
