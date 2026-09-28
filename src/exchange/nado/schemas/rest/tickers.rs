use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::price_data::TickerData, base_data::InstrumentType,
};

use crate::exchange::nado::api_utils::nado_product_to_cli;

/// Archive 24h ticker, keyed by `ticker_id`; `last_price` is 0 before a market's first trade.
#[derive(Clone, Debug, Deserialize)]
pub struct RestTickerNado {
    pub product_id: u32,
    pub ticker_id: String,
    pub base_currency: String,
    pub quote_currency: String,
    pub last_price: f64,
    pub base_volume: f64,
    pub quote_volume: f64,
    pub price_change_percent_24h: f64,
}

impl RestTickerNado {
    pub fn inst(&self) -> String {
        nado_product_to_cli(self.product_id)
    }

    pub fn into_ticker_data(self, inst_type: InstrumentType, timestamp: u64) -> Option<TickerData> {
        (self.last_price > 0.0).then(|| TickerData {
            timestamp,
            inst: self.inst(),
            inst_type,
            price: self.last_price,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const RAW: &str = r#"{"BTC-PERP_USDT0":{"product_id":2,"ticker_id":"BTC-PERP_USDT0","base_currency":"BTC-PERP",
        "quote_currency":"USDT0","last_price":82924.0,"base_volume":1111.98545,"quote_volume":93276126.31789081,
        "price_change_percent_24h":-2.1115333968040697},
        "ANSEM-PERP_USDT0":{"product_id":184,"ticker_id":"ANSEM-PERP_USDT0","base_currency":"ANSEM-PERP",
        "quote_currency":"USDT0","last_price":0.0,"base_volume":0.0,"quote_volume":0.0,
        "price_change_percent_24h":-9.53686098250909}}"#;

    fn tickers() -> HashMap<String, RestTickerNado> {
        serde_json::from_str(RAW).unwrap()
    }

    #[test]
    fn ticker_maps_to_ticker_data() {
        let btc = tickers().remove("BTC-PERP_USDT0").unwrap();
        assert_eq!(btc.base_currency, "BTC-PERP");

        let ticker = btc.into_ticker_data(InstrumentType::Perpetual, 7).unwrap();
        assert_eq!(
            (ticker.inst.as_str(), ticker.price, ticker.timestamp),
            ("@2", 82924.0, 7)
        );
        assert_eq!(ticker.inst_type, InstrumentType::Perpetual);
    }

    #[test]
    fn untraded_market_has_no_ticker() {
        let ansem = tickers().remove("ANSEM-PERP_USDT0").unwrap();

        assert!(
            ansem
                .into_ticker_data(InstrumentType::Perpetual, 7)
                .is_none()
        );
    }

    #[test]
    fn spot_ticker_parses() {
        let raw = r#"{"product_id":1,"ticker_id":"KBTC_USDT0","base_currency":"KBTC","quote_currency":"USDT0",
            "last_price":82878.0,"base_volume":0.64545,"quote_volume":54274.08578321814,
            "price_change_percent_24h":-2.1118928211166033}"#;
        let ticker = serde_json::from_str::<RestTickerNado>(raw)
            .unwrap()
            .into_ticker_data(InstrumentType::Spot, 1)
            .unwrap();

        assert_eq!((ticker.inst.as_str(), ticker.price), ("@1", 82878.0));
        assert_eq!(ticker.inst_type, InstrumentType::Spot);
    }
}
