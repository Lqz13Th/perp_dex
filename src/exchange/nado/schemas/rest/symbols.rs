use serde::Deserialize;
use std::collections::HashMap;

use extrema_infra::arch::market_assets::{
    api_data::utils_data::InstrumentInfo,
    base_data::{InstrumentStatus, InstrumentType},
};

use crate::exchange::nado::api_utils::{nado_product_to_cli, nado_x18_to_f64};

/// `symbols` keyed by symbol; the quote product USDT0 is not listed.
#[derive(Clone, Debug, Deserialize)]
pub struct RestSymbolsNado {
    pub symbols: HashMap<String, SymbolNado>,
}

impl RestSymbolsNado {
    pub fn into_products(self) -> Vec<SymbolNado> {
        let mut products: Vec<SymbolNado> = self.symbols.into_values().collect();
        products.sort_by_key(|p| p.product_id);
        products
    }
}

/// Every numeric field is x18 fixed point; `min_size` is a USDT0 notional.
#[derive(Clone, Debug, Deserialize)]
pub struct SymbolNado {
    #[serde(rename = "type")]
    pub product_type: String,
    pub product_id: u32,
    pub symbol: String,
    pub price_increment_x18: String,
    pub size_increment: String,
    pub min_size: String,
    pub maker_fee_rate_x18: String,
    pub taker_fee_rate_x18: String,
    pub long_weight_initial_x18: String,
    pub long_weight_maintenance_x18: String,
    #[serde(default)]
    pub max_open_interest_x18: Option<String>,
    pub trading_status: String,
    pub isolated_only: bool,
}

impl SymbolNado {
    pub fn inst(&self) -> String {
        nado_product_to_cli(self.product_id)
    }

    pub fn inst_type(&self) -> InstrumentType {
        match self.product_type.as_str() {
            "perp" => InstrumentType::Perpetual,
            "spot" => InstrumentType::Spot,
            _ => InstrumentType::Unknown,
        }
    }

    /// `soft_reduce_only` is the off-hours state of scheduled markets.
    pub fn state(&self) -> InstrumentStatus {
        match self.trading_status.as_str() {
            "live" => InstrumentStatus::Live,
            "post_only" | "soft_reduce_only" => InstrumentStatus::Suspend,
            "reduce_only" => InstrumentStatus::Delisting,
            "not_tradable" => InstrumentStatus::Closed,
            _ => InstrumentStatus::Unknown,
        }
    }

    /// Perp initial margin is `1 - long_weight_initial`.
    pub fn max_leverage(&self) -> Option<u32> {
        let margin = 1.0 - nado_x18_to_f64(&self.long_weight_initial_x18);

        (self.inst_type() == InstrumentType::Perpetual && margin > 0.0 && margin < 1.0)
            .then(|| (1.0 / margin).round() as u32)
    }
}

impl From<SymbolNado> for InstrumentInfo {
    fn from(d: SymbolNado) -> Self {
        let lot_size = nado_x18_to_f64(&d.size_increment);
        let min_notional = nado_x18_to_f64(&d.min_size);

        InstrumentInfo {
            inst: d.inst(),
            inst_code: Some(d.symbol.clone()),
            inst_type: d.inst_type(),
            lot_size,
            tick_size: nado_x18_to_f64(&d.price_increment_x18),
            min_lmt_size: lot_size,
            max_lmt_size: f64::MAX,
            min_mkt_size: lot_size,
            max_mkt_size: f64::MAX,
            max_leverage: d.max_leverage(),
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

    const BTC: &str = r#"{"type":"perp","product_id":2,"symbol":"BTC-PERP","price_increment_x18":"1000000000000000000",
        "size_increment":"50000000000000","min_size":"100000000000000000000","maker_fee_rate_x18":"100000000000000",
        "taker_fee_rate_x18":"350000000000000","long_weight_initial_x18":"980000000000000000",
        "long_weight_maintenance_x18":"990000000000000000","max_open_interest_x18":"165000000000000000000000000",
        "trading_status":"live","isolated_only":false}"#;

    const NVDA: &str = r#"{"type":"perp","product_id":112,"symbol":"NVDA-PERP","price_increment_x18":"10000000000000000",
        "size_increment":"10000000000000000","min_size":"100000000000000000000","maker_fee_rate_x18":"100000000000000",
        "taker_fee_rate_x18":"350000000000000","long_weight_initial_x18":"950000000000000000",
        "long_weight_maintenance_x18":"975000000000000000","max_open_interest_x18":"5000000000000000000000000",
        "trading_status":"live","isolated_only":false}"#;

    const KBTC: &str = r#"{"type":"spot","product_id":1,"symbol":"KBTC","price_increment_x18":"1000000000000000000",
        "size_increment":"50000000000000","min_size":"100000000000000000000","maker_fee_rate_x18":"100000000000000",
        "taker_fee_rate_x18":"350000000000000","long_weight_initial_x18":"950000000000000000",
        "long_weight_maintenance_x18":"970000000000000000","max_open_interest_x18":null,"trading_status":"live",
        "isolated_only":false}"#;

    const WAMZNX: &str = r#"{"type":"spot","product_id":145,"symbol":"wAMZNx","price_increment_x18":"10000000000000000",
        "size_increment":"2000000000000000","min_size":"100000000000000000000","maker_fee_rate_x18":"100000000000000",
        "taker_fee_rate_x18":"350000000000000","long_weight_initial_x18":"0","long_weight_maintenance_x18":"0",
        "max_open_interest_x18":null,"trading_status":"not_tradable","isolated_only":false}"#;

    fn symbol(raw: &str) -> SymbolNado {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn perp_maps_to_instrument_info() {
        let info = InstrumentInfo::from(symbol(BTC));

        assert_eq!(info.inst, "@2");
        assert_eq!(info.inst_code.as_deref(), Some("BTC-PERP"));
        assert_eq!(info.inst_type, InstrumentType::Perpetual);
        assert_eq!(info.tick_size, 1.0);
        assert_eq!(info.lot_size, 0.00005);
        assert_eq!(info.min_lmt_size, 0.00005);
        assert_eq!(info.max_lmt_size, f64::MAX);
        assert_eq!(info.min_notional, Some(100.0));
        assert_eq!(info.max_leverage, Some(50));
        assert_eq!(info.state, InstrumentStatus::Live);
    }

    #[test]
    fn stock_perp_has_its_own_increments() {
        let info = InstrumentInfo::from(symbol(NVDA));

        assert_eq!(
            (info.inst.as_str(), info.inst_code.as_deref()),
            ("@112", Some("NVDA-PERP"))
        );
        assert_eq!((info.tick_size, info.lot_size), (0.01, 0.01));
        assert_eq!(info.max_leverage, Some(20));
    }

    #[test]
    fn spot_has_no_leverage() {
        let info = InstrumentInfo::from(symbol(KBTC));

        assert_eq!(info.inst, "@1");
        assert_eq!(info.inst_type, InstrumentType::Spot);
        assert_eq!(info.max_leverage, None);
        assert_eq!(symbol(KBTC).max_open_interest_x18, None);

        let closed = InstrumentInfo::from(symbol(WAMZNX));
        assert_eq!(closed.state, InstrumentStatus::Closed);
        assert_eq!(closed.max_leverage, None);
    }

    #[test]
    fn trading_status_maps_to_state() {
        for (status, state) in [
            ("post_only", InstrumentStatus::Suspend),
            ("soft_reduce_only", InstrumentStatus::Suspend),
            ("reduce_only", InstrumentStatus::Delisting),
            ("not_tradable", InstrumentStatus::Closed),
            ("halted", InstrumentStatus::Unknown),
        ] {
            let raw = BTC.replace(
                r#""trading_status":"live""#,
                &format!(r#""trading_status":"{status}""#),
            );
            assert_eq!(symbol(&raw).state(), state, "{status}");
        }
    }

    #[test]
    fn symbols_response_is_sorted_by_product() {
        let raw = format!(r#"{{"symbols":{{"NVDA-PERP":{NVDA},"KBTC":{KBTC},"BTC-PERP":{BTC}}}}}"#);
        let products = serde_json::from_str::<RestSymbolsNado>(&raw)
            .unwrap()
            .into_products();

        let ids: Vec<u32> = products.iter().map(|p| p.product_id).collect();
        assert_eq!(ids, vec![1, 2, 112]);
    }
}
