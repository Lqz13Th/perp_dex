use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::utils_data::InstrumentInfo,
    base_data::{InstrumentStatus, InstrumentType},
};

use crate::exchange::grvt::api_utils::grvt_inst_to_cli;

/// `asset_class` is `UNSPECIFIED` for every live instrument, so stocks are not told apart from crypto.
#[derive(Clone, Debug, Deserialize)]
pub struct InstrumentGrvt {
    pub instrument: String,
    pub instrument_hash: String,
    pub base: String,
    pub quote: String,
    pub kind: String,
    #[serde(default)]
    pub venues: Vec<String>,
    #[serde(default)]
    pub settlement_period: Option<String>,
    pub base_decimals: i32,
    pub quote_decimals: i32,
    pub tick_size: String,
    pub min_size: String,
    pub create_time: String,
    #[serde(default)]
    pub max_position_size: Option<String>,
    #[serde(default)]
    pub funding_interval_hours: Option<u32>,
    #[serde(default)]
    pub adjusted_funding_rate_cap: Option<String>,
    #[serde(default)]
    pub adjusted_funding_rate_floor: Option<String>,
    pub min_notional: String,
    #[serde(default)]
    pub asset_class: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

impl InstrumentGrvt {
    pub fn inst(&self) -> String {
        grvt_inst_to_cli(&self.instrument)
    }

    pub fn inst_type(&self) -> InstrumentType {
        match self.kind.as_str() {
            "PERPETUAL" => InstrumentType::Perpetual,
            "FUTURE" => InstrumentType::Futures,
            "CALL" | "PUT" => InstrumentType::Options,
            _ => InstrumentType::Unknown,
        }
    }

    pub fn state(&self) -> InstrumentStatus {
        match self.status.as_deref() {
            Some("ACTIVE") => InstrumentStatus::Live,
            Some("DELISTED") => InstrumentStatus::Closed,
            _ => InstrumentStatus::Unknown,
        }
    }
}

impl From<InstrumentGrvt> for InstrumentInfo {
    fn from(d: InstrumentGrvt) -> Self {
        let lot_size = d.min_size.parse().unwrap_or_default();
        let max_size = d
            .max_position_size
            .as_deref()
            .and_then(|size| size.parse().ok())
            .unwrap_or(f64::MAX);
        let min_notional: f64 = d.min_notional.parse().unwrap_or_default();

        InstrumentInfo {
            inst: d.inst(),
            inst_code: Some(d.instrument_hash.clone()),
            inst_type: d.inst_type(),
            lot_size,
            tick_size: d.tick_size.parse().unwrap_or_default(),
            min_lmt_size: lot_size,
            max_lmt_size: max_size,
            min_mkt_size: lot_size,
            max_mkt_size: max_size,
            max_leverage: None,
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

    const NVDA: &str = r#"{"instrument":"NVDA_USDT_Perp","instrument_hash":"0x036e01","base":"NVDA",
        "quote":"USDT","kind":"PERPETUAL","venues":["ORDERBOOK","RFQ"],"settlement_period":"PERPETUAL",
        "base_decimals":9,"quote_decimals":6,"tick_size":"0.01","min_size":"0.01",
        "create_time":"1787802370345446469","max_position_size":"88888.0","funding_interval_hours":8,
        "adjusted_funding_rate_cap":"2.0","adjusted_funding_rate_floor":"-2.0","min_notional":"5.0",
        "asset_class":"UNSPECIFIED","status":"ACTIVE"}"#;

    const IP: &str = r#"{"instrument":"IP_USDT_Perp","instrument_hash":"0x032801","base":"IP",
        "quote":"USDT","kind":"PERPETUAL","venues":["ORDERBOOK","RFQ"],"settlement_period":"PERPETUAL",
        "base_decimals":6,"quote_decimals":6,"tick_size":"0.0001","min_size":"0.1",
        "create_time":"1787802370345433429","max_position_size":"627353.0","funding_interval_hours":4,
        "adjusted_funding_rate_cap":"0.0","adjusted_funding_rate_floor":"0.0","min_notional":"5.0",
        "asset_class":"UNSPECIFIED","status":"DELISTED"}"#;

    fn instrument(raw: &str) -> InstrumentGrvt {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn stock_perp_maps_to_instrument_info() {
        let raw = instrument(NVDA);
        assert_eq!((raw.base.as_str(), raw.quote.as_str()), ("NVDA", "USDT"));
        assert_eq!(raw.funding_interval_hours, Some(8));

        let info = InstrumentInfo::from(raw);
        assert_eq!(info.inst, "NVDA_USDT_PERP");
        assert_eq!(info.inst_code.as_deref(), Some("0x036e01"));
        assert_eq!(info.inst_type, InstrumentType::Perpetual);
        assert_eq!(info.tick_size, 0.01);
        assert_eq!(info.lot_size, 0.01);
        assert_eq!(info.min_lmt_size, 0.01);
        assert_eq!(info.max_lmt_size, 88888.0);
        assert_eq!(info.max_mkt_size, 88888.0);
        assert_eq!(info.min_notional, Some(5.0));
        assert_eq!(info.max_leverage, None);
        assert_eq!(info.state, InstrumentStatus::Live);
    }

    #[test]
    fn delisted_perp_is_closed() {
        let info = InstrumentInfo::from(instrument(IP));

        assert_eq!(info.inst, "IP_USDT_PERP");
        assert_eq!(info.tick_size, 0.0001);
        assert_eq!(info.state, InstrumentStatus::Closed);
    }

    #[test]
    fn documented_fields_only_still_map() {
        let raw = r#"{"instrument":"BTC_USDT_Fut_20Oct23","instrument_hash":"0x030102","base":"BTC",
            "quote":"USDT","kind":"FUTURE","venues":["ORDERBOOK"],"base_decimals":9,"quote_decimals":6,
            "tick_size":"0.1","min_size":"0.001","create_time":"1697788800000000000","min_notional":"100.0"}"#;

        let raw = instrument(raw);
        assert_eq!(raw.inst(), "BTC_USDT_Fut_20Oct23");
        assert_eq!(raw.inst_type(), InstrumentType::Futures);
        assert_eq!(raw.state(), InstrumentStatus::Unknown);
        assert_eq!(InstrumentInfo::from(raw).max_lmt_size, f64::MAX);
    }

    #[test]
    fn unknown_status_and_kind_are_unknown() {
        let raw = instrument(
            &NVDA
                .replace(r#""status":"ACTIVE""#, r#""status":"HALTED""#)
                .replace(r#""kind":"PERPETUAL""#, r#""kind":"STABLE_PERP""#),
        );

        assert_eq!(raw.state(), InstrumentStatus::Unknown);
        assert_eq!(raw.inst_type(), InstrumentType::Unknown);
    }
}
