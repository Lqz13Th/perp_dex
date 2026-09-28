use serde_json::{Map, json};

use extrema_infra::{
    arch::market_assets::api_general::ts_to_micros,
    prelude::{InfraError, InfraResult, InstrumentType, LobFrequency, LobParam, TradesParam},
};

use super::config_assets::NADO_MAX_BOOK_LEVELS;

const NADO_X18_DECIMALS: usize = 18;

/// Nado frames carry only the product id, so every Nado instrument is `@<product_id>`.
pub fn nado_product_to_cli(product_id: u32) -> String {
    format!("@{product_id}")
}

pub fn cli_to_nado_product_id(inst: &str) -> InfraResult<u32> {
    inst.strip_prefix('@')
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| {
            InfraError::ApiCliError(format!("Nado instruments are @<product_id>, got {inst}"))
        })
}

/// `"1800000000000000"` -> `"0.0018"`: the exact decimal of an x18 fixed-point integer.
pub fn nado_x18_to_decimal(raw: &str) -> Option<String> {
    let (sign, digits) = match raw.strip_prefix('-') {
        Some(digits) => ("-", digits),
        None => ("", raw),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }

    let padded = format!(
        "{:0>width$}",
        digits.trim_start_matches('0'),
        width = NADO_X18_DECIMALS + 1
    );
    let (int, frac) = padded.split_at(padded.len() - NADO_X18_DECIMALS);
    let frac = frac.trim_end_matches('0');

    Some(match (int, frac) {
        ("0", "") => "0".to_string(),
        (int, "") => format!("{sign}{int}"),
        (int, frac) => format!("{sign}{int}.{frac}"),
    })
}

/// Rounds the exact decimal once, where dividing a parsed f64 by 1e18 rounds twice.
pub fn nado_x18_to_f64(raw: &str) -> f64 {
    nado_x18_to_decimal(raw)
        .and_then(|decimal| decimal.parse().ok())
        .unwrap_or_default()
}

/// Nado stamps events in nanoseconds.
pub fn nado_ns_to_micros(timestamp_ns: u64) -> u64 {
    ts_to_micros(timestamp_ns / 1_000)
}

pub fn ws_subscribe_msg_nado(stream: &str, product_id: Option<u32>) -> String {
    let mut stream_msg = Map::new();
    stream_msg.insert("type".into(), json!(stream));
    if let Some(product_id) = product_id {
        stream_msg.insert("product_id".into(), json!(product_id));
    }

    json!({
        "method": "subscribe",
        "stream": stream_msg,
        "id": 1,
    })
    .to_string()
}

pub fn nado_lob_stream(lob_param: &Option<LobParam>) -> InfraResult<&'static str> {
    match lob_param {
        None
        | Some(LobParam::Incremental {
            depth: None,
            frequency: None,
        }) => Ok("book_depth"),
        Some(LobParam::Bbo {
            frequency: None | Some(LobFrequency::Realtime),
        }) => Ok("best_bid_offer"),
        Some(param) => Err(InfraError::ApiCliError(format!(
            "Nado pushes 50ms book_depth diffs without a snapshot, or a realtime best_bid_offer; unsupported {:?}",
            param
        ))),
    }
}

pub fn nado_trades_stream(trades_param: &Option<TradesParam>) -> InfraResult<&'static str> {
    match trades_param {
        None | Some(TradesParam::AllTrades) => Ok("trade"),
        Some(TradesParam::AggTrades) => Err(InfraError::ApiCliError(
            "Nado publishes individual matches only".into(),
        )),
    }
}

pub(crate) fn nado_orderbook_depth(depth: usize) -> InfraResult<usize> {
    match depth {
        0 => Ok(NADO_MAX_BOOK_LEVELS),
        1..=NADO_MAX_BOOK_LEVELS => Ok(depth),
        depth => Err(InfraError::ApiCliError(format!(
            "Nado orderbook supports 1 to {NADO_MAX_BOOK_LEVELS} levels: {depth}"
        ))),
    }
}

/// The archive's `market` filter; its tickers carry no product type.
pub(crate) fn nado_ticker_market(inst_type: &InstrumentType) -> Option<&'static str> {
    match inst_type {
        InstrumentType::Perpetual => Some("perp"),
        InstrumentType::Spot => Some("spot"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn product_ids_round_trip_through_cli_names() {
        for product_id in [0, 1, 2, 112, 188] {
            let inst = nado_product_to_cli(product_id);
            assert_eq!(inst, format!("@{product_id}"));
            assert_eq!(cli_to_nado_product_id(&inst).unwrap(), product_id);
        }
    }

    #[test]
    fn non_product_instruments_are_rejected() {
        for inst in [
            "BTC-PERP",
            "BTC_USDT0_PERP",
            "@",
            "@-2",
            "@2.0",
            "2",
            "@BTC",
        ] {
            assert!(cli_to_nado_product_id(inst).is_err(), "{inst}");
        }
    }

    #[test]
    fn x18_integers_become_exact_decimals() {
        for (raw, decimal) in [
            ("82923000000000000000000", "82923"),
            ("1800000000000000", "0.0018"),
            ("223350000000000000000", "223.35"),
            ("1000000000000000000", "1"),
            ("1", "0.000000000000000001"),
            ("0", "0"),
            ("000", "0"),
            ("-0", "0"),
            ("-300000000000000", "-0.0003"),
            ("00076250000000000000", "0.07625"),
            (
                "170141183460469231731687303715884105727",
                "170141183460469231731.687303715884105727",
            ),
        ] {
            assert_eq!(nado_x18_to_decimal(raw).as_deref(), Some(decimal), "{raw}");
        }
    }

    #[test]
    fn malformed_x18_values_are_rejected() {
        for raw in ["", "-", "1.5", "1e18", " 1", "abc", "+1"] {
            assert_eq!(nado_x18_to_decimal(raw), None, "{raw}");
            assert_eq!(nado_x18_to_f64(raw), 0.0);
        }
    }

    #[test]
    fn x18_to_f64_rounds_once() {
        let raw = "590310000000000000000";

        assert_eq!(nado_x18_to_f64(raw), 590.31);
        assert_ne!(raw.parse::<f64>().unwrap() / 1e18, 590.31);
        assert_eq!(nado_x18_to_f64("1778950000000000000"), 1.77895);
        assert_eq!(nado_x18_to_f64("-43191576457943000"), -0.043191576457943);
    }

    #[test]
    fn nanoseconds_become_micros() {
        assert_eq!(
            nado_ns_to_micros(1_790_586_212_414_817_593),
            1_790_586_212_414_817
        );
        assert_eq!(nado_ns_to_micros(0), 0);
    }

    #[test]
    fn subscribe_msg_carries_the_product_when_given() {
        let msg: Value =
            serde_json::from_str(&ws_subscribe_msg_nado("book_depth", Some(2))).unwrap();
        assert_eq!(
            msg,
            json!({"method":"subscribe","stream":{"type":"book_depth","product_id":2},"id":1})
        );

        let msg: Value = serde_json::from_str(&ws_subscribe_msg_nado("all_bbo", None)).unwrap();
        assert_eq!(
            msg,
            json!({"method":"subscribe","stream":{"type":"all_bbo"},"id":1})
        );
    }

    #[test]
    fn lob_params_map_to_streams() {
        assert_eq!(nado_lob_stream(&None).unwrap(), "book_depth");
        assert_eq!(
            nado_lob_stream(&Some(LobParam::Incremental {
                depth: None,
                frequency: None
            }))
            .unwrap(),
            "book_depth"
        );
        assert_eq!(
            nado_lob_stream(&Some(LobParam::Bbo { frequency: None })).unwrap(),
            "best_bid_offer"
        );
        assert_eq!(
            nado_lob_stream(&Some(LobParam::Bbo {
                frequency: Some(LobFrequency::Realtime)
            }))
            .unwrap(),
            "best_bid_offer"
        );
    }

    #[test]
    fn unsupported_lob_params_are_rejected() {
        let params = [
            LobParam::Snapshot {
                depth: None,
                frequency: None,
            },
            LobParam::Snapshot {
                depth: Some(20),
                frequency: None,
            },
            LobParam::Incremental {
                depth: Some(100),
                frequency: None,
            },
            LobParam::Incremental {
                depth: None,
                frequency: Some(LobFrequency::Ms100),
            },
            LobParam::Bbo {
                frequency: Some(LobFrequency::Ms100),
            },
        ];

        for param in params {
            let err = nado_lob_stream(&Some(param.clone())).unwrap_err();
            assert!(matches!(err, InfraError::ApiCliError(_)), "{param:?}");
        }
    }

    #[test]
    fn trades_are_individual_only() {
        assert_eq!(nado_trades_stream(&None).unwrap(), "trade");
        assert_eq!(
            nado_trades_stream(&Some(TradesParam::AllTrades)).unwrap(),
            "trade"
        );
        assert!(nado_trades_stream(&Some(TradesParam::AggTrades)).is_err());
    }

    #[test]
    fn orderbook_depth_is_bounded() {
        assert_eq!(nado_orderbook_depth(0).unwrap(), 100);
        assert_eq!(nado_orderbook_depth(1).unwrap(), 1);
        assert_eq!(nado_orderbook_depth(100).unwrap(), 100);
        assert!(nado_orderbook_depth(101).is_err());
    }

    #[test]
    fn ticker_market_follows_the_instrument_type() {
        assert_eq!(nado_ticker_market(&InstrumentType::Perpetual), Some("perp"));
        assert_eq!(nado_ticker_market(&InstrumentType::Spot), Some("spot"));
        assert_eq!(nado_ticker_market(&InstrumentType::Futures), None);
    }
}
