use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::price_data::TickerData, api_general::ts_to_micros, base_data::InstrumentType,
};

use crate::exchange::aster::api_utils::aster_inst_to_cli;

#[derive(Clone, Debug, Deserialize)]
pub struct RestTickerAster {
    pub symbol: String,
    pub price: String,
    pub time: u64,
}

impl From<RestTickerAster> for TickerData {
    fn from(d: RestTickerAster) -> Self {
        TickerData {
            timestamp: ts_to_micros(d.time),
            inst: aster_inst_to_cli(&d.symbol),
            inst_type: InstrumentType::Perpetual,
            price: d.price.parse().unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_price_ticker() {
        let raw = r#"{"symbol":"MUUSD1","price":"141.2300","time":1790577846150}"#;
        let ticker = TickerData::from(serde_json::from_str::<RestTickerAster>(raw).unwrap());

        assert_eq!(ticker.inst, "MU_USD1_PERP");
        assert_eq!(ticker.inst_type, InstrumentType::Perpetual);
        assert_eq!(ticker.price, 141.23);
        assert_eq!(ticker.timestamp, 1_790_577_846_150_000);
    }
}
