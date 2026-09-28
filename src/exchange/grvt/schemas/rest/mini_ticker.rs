use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::price_data::{MarkPriceData, TickerData},
    api_general::de_u64_from_string_or_number,
    base_data::InstrumentType,
};

use crate::exchange::grvt::api_utils::{grvt_inst_to_cli, grvt_ns_to_micros};

#[derive(Clone, Debug, Deserialize)]
pub struct MiniTickerGrvt {
    #[serde(default, deserialize_with = "de_u64_from_string_or_number")]
    pub event_time: u64,
    pub instrument: String,
    #[serde(default)]
    pub mark_price: Option<String>,
    #[serde(default)]
    pub index_price: Option<String>,
    #[serde(default)]
    pub last_price: Option<String>,
    #[serde(default)]
    pub last_size: Option<String>,
    #[serde(default)]
    pub mid_price: Option<String>,
    #[serde(default)]
    pub best_bid_price: Option<String>,
    #[serde(default)]
    pub best_bid_size: Option<String>,
    #[serde(default)]
    pub best_ask_price: Option<String>,
    #[serde(default)]
    pub best_ask_size: Option<String>,
}

impl MiniTickerGrvt {
    pub fn inst(&self) -> String {
        grvt_inst_to_cli(&self.instrument)
    }

    pub fn into_ticker_data(self) -> Option<TickerData> {
        let price = self.last_price.as_deref()?.parse().ok()?;

        Some(TickerData {
            timestamp: grvt_ns_to_micros(self.event_time),
            inst: self.inst(),
            inst_type: InstrumentType::Perpetual,
            price,
        })
    }

    pub fn into_mark_price_data(self) -> Option<MarkPriceData> {
        let mark_price = self.mark_price.as_deref()?.parse().ok()?;

        Some(MarkPriceData {
            timestamp: grvt_ns_to_micros(self.event_time),
            inst: self.inst(),
            inst_type: InstrumentType::Perpetual,
            mark_price,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NVDA: &str = r#"{"event_time":"1790585907194591621","instrument":"NVDA_USDT_Perp",
        "mark_price":"223.559156282","index_price":"223.391303987","last_price":"223.56","last_size":"2.26",
        "mid_price":"223.56","best_bid_price":"223.52","best_bid_size":"108.66","best_ask_price":"223.6",
        "best_ask_size":"56.1"}"#;

    fn mini(raw: &str) -> MiniTickerGrvt {
        serde_json::from_str(raw).unwrap()
    }

    #[test]
    fn ticker_and_mark_price_are_timed_by_the_venue() {
        let ticker = mini(NVDA).into_ticker_data().unwrap();
        let mark = mini(NVDA).into_mark_price_data().unwrap();

        assert_eq!(
            (ticker.inst.as_str(), ticker.price, ticker.timestamp),
            ("NVDA_USDT_PERP", 223.56, 1_790_585_907_194_591)
        );
        assert_eq!(ticker.inst_type, InstrumentType::Perpetual);
        assert_eq!(
            (mark.mark_price, mark.timestamp),
            (223.559156282, 1_790_585_907_194_591)
        );
    }

    #[test]
    fn delisted_book_keeps_its_last_and_mark_price() {
        let raw = mini(
            r#"{"event_time":"1790585905163690462","instrument":"IP_USDT_Perp","mark_price":"0.310934948",
            "index_price":"0.310828469","last_price":"0.3108","last_size":"104.5","mid_price":"0.0",
            "best_bid_price":"0.0","best_bid_size":"0.0","best_ask_price":"0.0","best_ask_size":"0.0"}"#,
        );

        assert_eq!(raw.best_bid_price.as_deref(), Some("0.0"));
        assert_eq!(raw.clone().into_ticker_data().unwrap().price, 0.3108);
        assert_eq!(raw.into_mark_price_data().unwrap().mark_price, 0.310934948);
    }

    #[test]
    fn missing_or_null_prices_give_no_data() {
        let raw = mini(r#"{"instrument":"NEW_USDT_Perp","last_price":null}"#);

        assert_eq!(raw.event_time, 0);
        assert!(raw.clone().into_ticker_data().is_none());
        assert!(raw.into_mark_price_data().is_none());
    }
}
