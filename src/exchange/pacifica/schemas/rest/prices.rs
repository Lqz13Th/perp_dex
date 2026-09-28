use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::price_data::{MarkPriceData, TickerData},
    api_general::ts_to_micros,
    base_data::InstrumentType,
};

use crate::exchange::pacifica::api_utils::{pacifica_inst_type, pacifica_symbol_to_cli};

/// `/info/prices` has no last trade price; tickers use `mid`, as infra's Hyperliquid tickers do.
#[derive(Clone, Debug, Deserialize)]
pub struct RestPricePacifica {
    pub symbol: String,
    pub mark: String,
    pub mid: String,
    pub oracle: String,
    pub funding: String,
    pub next_funding: String,
    /// USD notional.
    pub open_interest: String,
    /// USD notional.
    pub volume_24h: String,
    pub yesterday_price: String,
    pub timestamp: u64,
}

impl RestPricePacifica {
    pub fn inst(&self) -> String {
        pacifica_symbol_to_cli(&self.symbol)
    }

    pub fn inst_type(&self) -> InstrumentType {
        pacifica_inst_type(&self.symbol)
    }
}

impl From<RestPricePacifica> for TickerData {
    fn from(d: RestPricePacifica) -> Self {
        TickerData {
            timestamp: ts_to_micros(d.timestamp),
            inst: d.inst(),
            inst_type: d.inst_type(),
            price: d.mid.parse().unwrap_or_default(),
        }
    }
}

impl From<RestPricePacifica> for MarkPriceData {
    fn from(d: RestPricePacifica) -> Self {
        MarkPriceData {
            timestamp: ts_to_micros(d.timestamp),
            inst: d.inst(),
            inst_type: d.inst_type(),
            mark_price: d.mark.parse().unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NVDA: &str = r#"{"funding":"0.0000125","mark":"223.14","mid":"223.1","next_funding":"0.0000125",
        "open_interest":"1092.49","oracle":"223.128564","symbol":"NVDA","timestamp":1790585552699,
        "volume_24h":"49406.42296","yesterday_price":"225.02"}"#;

    const SPOT: &str = r#"{"funding":"0","mark":"118.365599","mid":"118.35","next_funding":"0",
        "open_interest":"0","oracle":"118.365599","symbol":"SOL-USDC","timestamp":1790585552699,
        "volume_24h":"122905.42342","yesterday_price":"123.99"}"#;

    fn price(raw: &str) -> RestPricePacifica {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn ticker_is_the_mid_at_exchange_time() {
        let ticker = TickerData::from(price(NVDA));

        assert_eq!(ticker.inst, "NVDA_USDC_PERP");
        assert_eq!(ticker.inst_type, InstrumentType::Perpetual);
        assert_eq!(ticker.price, 223.1);
        assert_eq!(ticker.timestamp, 1_790_585_552_699_000);
    }

    #[test]
    fn mark_price_is_the_mark() {
        let mark = MarkPriceData::from(price(NVDA));

        assert_eq!(mark.inst, "NVDA_USDC_PERP");
        assert_eq!(mark.mark_price, 223.14);
        assert_eq!(mark.timestamp, 1_790_585_552_699_000);
    }

    #[test]
    fn spot_price_is_a_spot_pair() {
        let raw = price(SPOT);
        assert_eq!(raw.oracle, "118.365599");

        let ticker = TickerData::from(raw);
        assert_eq!(
            (ticker.inst.as_str(), ticker.inst_type),
            ("SOL_USDC", InstrumentType::Spot)
        );
    }
}
