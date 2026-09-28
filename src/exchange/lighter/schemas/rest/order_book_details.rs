use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::{
        price_data::{MarkPriceData, TickerData},
        utils_data::InstrumentInfo,
    },
    api_general::value_to_f64,
    base_data::{InstrumentStatus, InstrumentType},
};

use crate::exchange::lighter::api_utils::lighter_market_to_cli;

#[derive(Clone, Debug, Deserialize)]
pub struct RestOrderBookDetailsLighter {
    #[serde(default)]
    pub order_book_details: Vec<OrderBookDetailLighter>,
    #[serde(default)]
    pub spot_order_book_details: Vec<OrderBookDetailLighter>,
}

impl RestOrderBookDetailsLighter {
    pub fn into_markets(self) -> impl Iterator<Item = OrderBookDetailLighter> {
        self.order_book_details
            .into_iter()
            .chain(self.spot_order_book_details)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct OrderBookDetailLighter {
    pub symbol: String,
    pub market_id: u16,
    pub market_type: String,
    pub status: String,
    #[serde(default)]
    pub is_frozen: Option<bool>,
    pub min_base_amount: String,
    pub min_quote_amount: String,
    pub supported_size_decimals: i32,
    pub supported_price_decimals: i32,
    pub supported_quote_decimals: i32,
    #[serde(default)]
    pub last_trade_price: serde_json::Value,
    #[serde(default)]
    pub mark_price: Option<String>,
    #[serde(default)]
    pub index_price: Option<String>,
    #[serde(default)]
    pub min_initial_margin_fraction: Option<u32>,
    #[serde(default)]
    pub strategy_index: Option<u32>,
    #[serde(default)]
    pub market_config: Option<MarketConfigLighter>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct MarketConfigLighter {
    #[serde(default)]
    pub force_reduce_only: bool,
    #[serde(default)]
    pub trading_hours: String,
    #[serde(default)]
    pub hidden: bool,
}

impl OrderBookDetailLighter {
    pub fn inst(&self) -> String {
        lighter_market_to_cli(self.market_id)
    }

    pub fn inst_type(&self) -> InstrumentType {
        match self.market_type.as_str() {
            "perp" => InstrumentType::Perpetual,
            "spot" => InstrumentType::Spot,
            _ => InstrumentType::Unknown,
        }
    }

    pub fn force_reduce_only(&self) -> bool {
        self.market_config
            .as_ref()
            .is_some_and(|config| config.force_reduce_only)
    }

    pub fn state(&self) -> InstrumentStatus {
        match self.status.as_str() {
            "active" if self.is_frozen == Some(true) || self.force_reduce_only() => {
                InstrumentStatus::Suspend
            },
            "active" => InstrumentStatus::Live,
            "inactive" => InstrumentStatus::Closed,
            _ => InstrumentStatus::Unknown,
        }
    }

    pub fn into_ticker_data(self, timestamp: u64) -> TickerData {
        TickerData {
            timestamp,
            inst: self.inst(),
            inst_type: self.inst_type(),
            price: value_to_f64(&self.last_trade_price),
        }
    }

    pub fn into_mark_price_data(self, timestamp: u64) -> Option<MarkPriceData> {
        let mark_price = self.mark_price.as_deref()?.parse().ok()?;

        Some(MarkPriceData {
            timestamp,
            inst: self.inst(),
            inst_type: self.inst_type(),
            mark_price,
        })
    }
}

impl From<OrderBookDetailLighter> for InstrumentInfo {
    fn from(d: OrderBookDetailLighter) -> Self {
        let lot_size = 10f64.powi(-d.supported_size_decimals);
        let tick_size = 10f64.powi(-d.supported_price_decimals);
        let min_notional: f64 = d.min_quote_amount.parse().unwrap_or_default();

        InstrumentInfo {
            inst: d.inst(),
            inst_code: Some(d.symbol.clone()),
            inst_type: d.inst_type(),
            lot_size,
            tick_size,
            min_lmt_size: d.min_base_amount.parse().unwrap_or(lot_size),
            max_lmt_size: f64::MAX,
            min_mkt_size: lot_size,
            max_mkt_size: f64::MAX,
            max_leverage: d
                .min_initial_margin_fraction
                .filter(|fraction| *fraction > 0)
                .map(|fraction| 10_000 / fraction),
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

    const NVDA: &str = r#"{"symbol":"NVDA","market_id":110,"market_type":"perp","status":"active",
        "taker_fee":"0.0000","maker_fee":"0.0000","min_base_amount":"0.025","min_quote_amount":"10.000000",
        "supported_size_decimals":3,"supported_price_decimals":3,"supported_quote_decimals":6,
        "is_frozen":false,"size_decimals":3,"price_decimals":3,"min_initial_margin_fraction":500,
        "mark_price":"223.383","index_price":"223.259","last_trade_price":223.41,
        "market_config":{"market_margin_mode":0,"force_reduce_only":false,"trading_hours":"","hidden":false},
        "strategy_index":5}"#;

    const SPOT: &str = r#"{"symbol":"ETH/USDC","market_id":2048,"market_type":"spot","status":"active",
        "min_base_amount":"0.0050","min_quote_amount":"10.000000","supported_size_decimals":4,
        "supported_price_decimals":2,"supported_quote_decimals":6,"is_frozen":false,"last_trade_price":2181}"#;

    fn detail(raw: &str) -> OrderBookDetailLighter {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn perp_maps_to_instrument_info() {
        let info = InstrumentInfo::from(detail(NVDA));

        assert_eq!(info.inst, "@110");
        assert_eq!(info.inst_code.as_deref(), Some("NVDA"));
        assert_eq!(info.inst_type, InstrumentType::Perpetual);
        assert_eq!(info.lot_size, 0.001);
        assert_eq!(info.tick_size, 0.001);
        assert_eq!(info.min_lmt_size, 0.025);
        assert_eq!(info.min_mkt_size, 0.001);
        assert_eq!(info.min_notional, Some(10.0));
        assert_eq!(info.max_leverage, Some(20));
        assert_eq!(info.state, InstrumentStatus::Live);
    }

    #[test]
    fn spot_without_margin_fields_still_maps() {
        let info = InstrumentInfo::from(detail(SPOT));

        assert_eq!(info.inst, "@2048");
        assert_eq!(info.inst_type, InstrumentType::Spot);
        assert_eq!(info.tick_size, 0.01);
        assert_eq!(info.max_leverage, None);
        assert!(detail(SPOT).into_mark_price_data(1).is_none());
    }

    #[test]
    fn reduce_only_frozen_and_inactive_markets_are_not_live() {
        let reduce_only = NVDA.replace(
            r#""force_reduce_only":false"#,
            r#""force_reduce_only":true"#,
        );
        let frozen = NVDA.replace(r#""is_frozen":false"#, r#""is_frozen":true"#);
        let inactive = NVDA.replace(r#""status":"active""#, r#""status":"inactive""#);

        assert_eq!(detail(&reduce_only).state(), InstrumentStatus::Suspend);
        assert_eq!(detail(&frozen).state(), InstrumentStatus::Suspend);
        assert_eq!(detail(&inactive).state(), InstrumentStatus::Closed);
    }

    #[test]
    fn robinhood_markets_without_is_frozen_parse() {
        let raw = NVDA.replace(r#""is_frozen":false,"#, "");
        let market = detail(&raw);

        assert_eq!(market.is_frozen, None);
        assert_eq!(market.state(), InstrumentStatus::Live);
    }

    #[test]
    fn ticker_and_mark_price_use_the_given_timestamp() {
        let ticker = detail(NVDA).into_ticker_data(7);
        let mark = detail(NVDA).into_mark_price_data(7).unwrap();

        assert_eq!(
            (ticker.inst.as_str(), ticker.price, ticker.timestamp),
            ("@110", 223.41, 7)
        );
        assert_eq!((mark.inst.as_str(), mark.mark_price), ("@110", 223.383));
        assert_eq!(detail(SPOT).into_ticker_data(0).price, 2181.0);
    }

    #[test]
    fn details_response_chains_perp_and_spot() {
        let raw = format!(
            r#"{{"code":200,"order_book_details":[{NVDA}],"spot_order_book_details":[{SPOT}]}}"#
        );
        let details: RestOrderBookDetailsLighter = serde_json::from_str(&raw).unwrap();

        let insts: Vec<String> = details.into_markets().map(|m| m.inst()).collect();
        assert_eq!(insts, vec!["@110", "@2048"]);
    }
}
