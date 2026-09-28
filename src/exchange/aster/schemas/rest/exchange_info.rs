use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::utils_data::InstrumentInfo,
    api_general::get_mills_timestamp,
    base_data::{InstrumentStatus, InstrumentType},
};

use crate::exchange::aster::api_utils::aster_inst_to_cli;

const ASTER_PERP_FAR_FUTURE_DELIVERY_MS: u64 = 3_786_912_000_000; // 2090-01-01T00:00:00Z

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct RestExchangeInfoAster {
    pub symbols: Vec<InstrumentInfoAster>,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct InstrumentInfoAster {
    pub symbol: String,
    pub pair: String,
    pub contractType: String,
    pub status: String,
    pub deliveryDate: u64,
    pub baseAsset: String,
    pub quoteAsset: String,
    pub marginAsset: String,
    pub pricePrecision: i32,
    pub quantityPrecision: i32,
    pub underlyingType: String,
    #[serde(default)]
    pub underlyingSubType: Vec<String>,
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub timeInForce: Vec<String>,
    filters: Vec<Filter>,
}

impl InstrumentInfoAster {
    /// Listed on a stock exchange (`nasdaq`, `hkstock`, `krstock`, `astock`); ETFs such as SPY are `ETF` only.
    pub fn is_stock(&self) -> bool {
        self.underlyingSubType.iter().any(|t| t == "STOCK")
    }
}

#[allow(non_camel_case_types)]
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "filterType")]
enum Filter {
    PRICE_FILTER(PriceFilter),
    LOT_SIZE(SizeFilter),
    MARKET_LOT_SIZE(SizeFilter),
    MIN_NOTIONAL(MinNotionalFilter),
    #[serde(other)]
    Other,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
struct PriceFilter {
    tickSize: String,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
struct SizeFilter {
    maxQty: String,
    minQty: String,
    stepSize: String,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
struct MinNotionalFilter {
    #[serde(default)]
    notional: Option<String>,
    #[serde(default)]
    minNotional: Option<String>,
}

impl From<InstrumentInfoAster> for InstrumentInfo {
    fn from(d: InstrumentInfoAster) -> Self {
        let mut tick_size = 0.0;
        let mut min_lmt_size = 0.0;
        let mut max_lmt_size = 0.0;
        let mut min_mkt_size = 0.0;
        let mut max_mkt_size = 0.0;
        let mut min_notional: f64 = 0.0;

        let mut lot_size_lmt = 0.0;
        let mut lot_size_mkt = 0.0;

        for f in d.filters.iter() {
            match f {
                Filter::PRICE_FILTER(pf) => {
                    tick_size = pf.tickSize.parse().unwrap_or_default();
                },
                Filter::LOT_SIZE(sf) => {
                    lot_size_lmt = sf.stepSize.parse::<f64>().unwrap_or_default();
                    min_lmt_size = sf.minQty.parse().unwrap_or_default();
                    max_lmt_size = sf.maxQty.parse().unwrap_or_default();
                },
                Filter::MARKET_LOT_SIZE(sf) => {
                    lot_size_mkt = sf.stepSize.parse::<f64>().unwrap_or_default();
                    min_mkt_size = sf.minQty.parse().unwrap_or_default();
                    max_mkt_size = sf.maxQty.parse().unwrap_or_default();
                },
                Filter::MIN_NOTIONAL(nf) => {
                    min_notional = min_notional.max(
                        nf.notional
                            .as_deref()
                            .or(nf.minNotional.as_deref())
                            .unwrap_or_default()
                            .parse::<f64>()
                            .unwrap_or_default(),
                    );
                },
                Filter::Other => {},
            };
        }

        InstrumentInfo {
            inst: aster_inst_to_cli(&d.symbol),
            inst_code: None,
            inst_type: match d.contractType.as_str() {
                "PERPETUAL" | "" => InstrumentType::Perpetual,
                "CURRENT_QUARTER" | "NEXT_QUARTER" | "CURRENT_MONTH" | "NEXT_MONTH" => {
                    InstrumentType::Futures
                },
                _ => InstrumentType::Unknown,
            },
            lot_size: lot_size_lmt.max(lot_size_mkt),
            tick_size,
            min_lmt_size,
            max_lmt_size,
            min_mkt_size,
            max_mkt_size,
            max_leverage: None,
            min_notional: (min_notional > 0.0).then_some(min_notional),
            contract_value: None,
            contract_multiplier: None,
            state: aster_status_to_instrument_status(
                &d.status,
                &d.contractType,
                d.deliveryDate,
                get_mills_timestamp(),
            ),
        }
    }
}

fn aster_status_to_instrument_status(
    status: &str,
    contract_type: &str,
    delivery_date_ms: u64,
    now_ms: u64,
) -> InstrumentStatus {
    match status {
        "SETTLING" => InstrumentStatus::Delisting,
        "TRADING"
            if contract_type == "PERPETUAL"
                && delivery_date_ms > now_ms
                && delivery_date_ms < ASTER_PERP_FAR_FUTURE_DELIVERY_MS =>
        {
            InstrumentStatus::Delisting
        },
        "TRADING" => InstrumentStatus::Live,
        "PENDING_TRADING" | "PRE_DELIVERING" => InstrumentStatus::PreOpen,
        "DELIVERING" | "PRE_SETTLE" => InstrumentStatus::Delisting,
        "CLOSE" => InstrumentStatus::Closed,
        _ => InstrumentStatus::Suspend,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NVDA: &str = r#"{
        "symbol":"NVDAUSDT","pair":"NVDAUSDT","contractType":"PERPETUAL",
        "deliveryDate":4133404800000,"onboardDate":1752562800000,"status":"TRADING",
        "baseAsset":"NVDA","quoteAsset":"USDT","marginAsset":"USDT",
        "pricePrecision":6,"quantityPrecision":2,
        "underlyingType":"COIN","underlyingSubType":["STOCK","Semiconductor"],
        "channel":"nasdaq","timeInForce":["GTC","IOC","GTX","HIDDEN"],
        "filters":[
            {"minPrice":"0.010000","maxPrice":"2000","filterType":"PRICE_FILTER","tickSize":"0.010000"},
            {"stepSize":"0.01","filterType":"LOT_SIZE","maxQty":"30000","minQty":"0.01"},
            {"stepSize":"0.01","filterType":"MARKET_LOT_SIZE","maxQty":"3000","minQty":"0.01"},
            {"limit":200,"filterType":"MAX_NUM_ORDERS"},
            {"notional":"5","filterType":"MIN_NOTIONAL"},
            {"multiplierDown":"0.9800","multiplierUp":"1.0200","filterType":"PERCENT_PRICE"}
        ]
    }"#;

    #[test]
    fn nvda_stock_perp_maps_to_instrument_info() {
        let raw: InstrumentInfoAster = serde_json::from_str(NVDA).unwrap();
        assert!(raw.is_stock());

        let info = InstrumentInfo::from(raw);
        assert_eq!(info.inst, "NVDA_USDT_PERP");
        assert_eq!(info.inst_type, InstrumentType::Perpetual);
        assert_eq!(info.tick_size, 0.01);
        assert_eq!(info.lot_size, 0.01);
        assert_eq!(info.min_lmt_size, 0.01);
        assert_eq!(info.max_lmt_size, 30000.0);
        assert_eq!(info.max_mkt_size, 3000.0);
        assert_eq!(info.min_notional, Some(5.0));
        assert_eq!(info.state, InstrumentStatus::Live);
    }

    #[test]
    fn pending_listing_without_contract_type_is_a_preopen_perp() {
        let raw = NVDA
            .replace(r#""contractType":"PERPETUAL""#, r#""contractType":"""#)
            .replace(r#""status":"TRADING""#, r#""status":"PENDING_TRADING""#);
        let info = InstrumentInfo::from(serde_json::from_str::<InstrumentInfoAster>(&raw).unwrap());

        assert_eq!(info.inst_type, InstrumentType::Perpetual);
        assert_eq!(info.state, InstrumentStatus::PreOpen);
    }

    #[test]
    fn statuses_map_like_binance_um() {
        let far = 4_133_404_800_000;
        let now = 1_790_000_000_000;

        assert_eq!(
            aster_status_to_instrument_status("TRADING", "PERPETUAL", far, now),
            InstrumentStatus::Live
        );
        assert_eq!(
            aster_status_to_instrument_status("TRADING", "PERPETUAL", now + 86_400_000, now),
            InstrumentStatus::Delisting
        );
        assert_eq!(
            aster_status_to_instrument_status("SETTLING", "PERPETUAL", far, now),
            InstrumentStatus::Delisting
        );
        assert_eq!(
            aster_status_to_instrument_status("CLOSE", "PERPETUAL", far, now),
            InstrumentStatus::Closed
        );
        assert_eq!(
            aster_status_to_instrument_status("HALT", "PERPETUAL", far, now),
            InstrumentStatus::Suspend
        );
    }

    #[test]
    fn etf_is_not_a_stock() {
        let raw = NVDA.replace(r#"["STOCK","Semiconductor"]"#, r#"["ETF"]"#);

        assert!(
            !serde_json::from_str::<InstrumentInfoAster>(&raw)
                .unwrap()
                .is_stock()
        );
    }
}
