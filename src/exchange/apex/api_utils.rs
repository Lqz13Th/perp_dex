use serde_json::json;
use tracing::warn;

use extrema_infra::prelude::{InfraError, InfraResult, LobParam, TradesParam};

use super::config_assets::APEX_MAX_BOOK_LEVELS;

const APEX_QUOTES: [&str; 2] = ["USDT", "USDC"];

/// `NVDAUSDT` -> `NVDA_USDT_PERP`.
pub fn apex_symbol_to_cli(symbol: &str) -> String {
    for quote in APEX_QUOTES {
        if let Some(base) = symbol.strip_suffix(quote) {
            if base.is_empty() {
                warn!("Invalid ApeX symbol: {}", symbol);
                return symbol.to_string();
            }
            return format!("{}_{}_PERP", base, quote);
        }
    }

    symbol.to_string()
}

/// `NVDA_USDT_PERP` -> `NVDAUSDT`.
pub fn cli_perp_to_apex_symbol(inst: &str) -> InfraResult<String> {
    inst.strip_suffix("_PERP")
        .and_then(|pair| pair.rsplit_once('_'))
        .filter(|(base, quote)| !base.is_empty() && APEX_QUOTES.contains(quote))
        .map(|(base, quote)| format!("{base}{quote}"))
        .ok_or_else(|| {
            InfraError::ApiCliError(format!(
                "ApeX instruments are <BASE>_<USDT|USDC>_PERP, got {inst}"
            ))
        })
}

/// Subscribes `<topic>.<SYMBOL>` for every instrument, or the bare topic without instruments.
pub fn ws_subscribe_msg_apex(topic: &str, insts: Option<&[String]>) -> InfraResult<String> {
    let args = match insts {
        Some(list) => list
            .iter()
            .map(|inst| Ok(format!("{}.{}", topic, cli_perp_to_apex_symbol(inst)?)))
            .collect::<InfraResult<Vec<String>>>()?,
        None => vec![topic.to_string()],
    };

    Ok(json!({
        "op": "subscribe",
        "args": args,
    })
    .to_string())
}

/// `{"op":"pong"}` carrying the client time; ApeX drops connections whose last pong is stale.
pub fn ws_pong_msg_apex(now_ms: u64) -> String {
    json!({
        "op": "pong",
        "args": [now_ms.to_string()],
    })
    .to_string()
}

pub fn apex_lob_topic(lob_param: &Option<LobParam>) -> InfraResult<&'static str> {
    match lob_param {
        None
        | Some(LobParam::Incremental {
            depth: None | Some(200),
            frequency: None,
        }) => Ok("orderBook200.H"),
        Some(LobParam::Incremental {
            depth: Some(25),
            frequency: None,
        }) => Ok("orderBook25.H"),
        Some(param) => Err(InfraError::ApiCliError(format!(
            "ApeX pushes a 25 or 200 level book then deltas; unsupported {:?}",
            param
        ))),
    }
}

pub fn apex_trades_topic(trades_param: &Option<TradesParam>) -> InfraResult<&'static str> {
    match trades_param {
        None | Some(TradesParam::AllTrades) => Ok("recentlyTrade.H"),
        Some(TradesParam::AggTrades) => Err(InfraError::ApiCliError(
            "ApeX publishes individual trades only".into(),
        )),
    }
}

pub(crate) fn apex_orderbook_limit(depth: usize) -> InfraResult<usize> {
    match depth {
        0 => Ok(APEX_MAX_BOOK_LEVELS),
        1..=APEX_MAX_BOOK_LEVELS => Ok(depth),
        depth => Err(InfraError::ApiCliError(format!(
            "ApeX orderbook supports 1 to {APEX_MAX_BOOK_LEVELS} levels: {depth}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use extrema_infra::prelude::LobFrequency;
    use serde_json::Value;

    use super::*;

    #[test]
    fn symbols_round_trip_through_cli_names() {
        let cases = [
            ("NVDAUSDT", "NVDA_USDT_PERP"),
            ("BTCUSDT", "BTC_USDT_PERP"),
            ("1000PEPEUSDT", "1000PEPE_USDT_PERP"),
            ("USDEUSDT", "USDE_USDT_PERP"),
            ("ETHUSDC", "ETH_USDC_PERP"),
            (
                "Knicks_Win_Against_Celtics_Dec2USDT",
                "Knicks_Win_Against_Celtics_Dec2_USDT_PERP",
            ),
        ];

        for (symbol, inst) in cases {
            assert_eq!(apex_symbol_to_cli(symbol), inst);
            assert_eq!(cli_perp_to_apex_symbol(inst).unwrap(), symbol);
        }
    }

    #[test]
    fn unknown_quote_is_left_as_is() {
        assert_eq!(apex_symbol_to_cli("USDT"), "USDT");
        assert_eq!(apex_symbol_to_cli("BTCUSD"), "BTCUSD");
    }

    #[test]
    fn malformed_instruments_are_rejected() {
        for inst in [
            "NVDAUSDT",
            "NVDA-USDT",
            "NVDA_USDT",
            "NVDA_PERP",
            "_USDT_PERP",
            "NVDA_USD_PERP",
            "NVDA__PERP",
        ] {
            assert!(cli_perp_to_apex_symbol(inst).is_err(), "{inst}");
        }
    }

    #[test]
    fn subscribe_msg_lists_every_instrument() {
        let insts = vec!["NVDA_USDT_PERP".to_string(), "BTC_USDT_PERP".to_string()];
        let msg: Value =
            serde_json::from_str(&ws_subscribe_msg_apex("orderBook25.H", Some(&insts)).unwrap())
                .unwrap();

        assert_eq!(
            msg,
            json!({"op":"subscribe","args":["orderBook25.H.NVDAUSDT","orderBook25.H.BTCUSDT"]})
        );
        assert_eq!(
            serde_json::from_str::<Value>(
                &ws_subscribe_msg_apex("instrumentInfo.all", None).unwrap()
            )
            .unwrap()["args"],
            json!(["instrumentInfo.all"])
        );
        assert!(ws_subscribe_msg_apex("orderBook25.H", Some(&["NVDAUSDT".to_string()])).is_err());
    }

    #[test]
    fn pong_carries_the_client_time() {
        assert_eq!(
            serde_json::from_str::<Value>(&ws_pong_msg_apex(1790585693937)).unwrap(),
            json!({"op":"pong","args":["1790585693937"]})
        );
    }

    #[test]
    fn lob_params_map_to_topics() {
        assert_eq!(apex_lob_topic(&None).unwrap(), "orderBook200.H");
        assert_eq!(
            apex_lob_topic(&Some(LobParam::Incremental {
                depth: None,
                frequency: None
            }))
            .unwrap(),
            "orderBook200.H"
        );
        assert_eq!(
            apex_lob_topic(&Some(LobParam::Incremental {
                depth: Some(200),
                frequency: None
            }))
            .unwrap(),
            "orderBook200.H"
        );
        assert_eq!(
            apex_lob_topic(&Some(LobParam::Incremental {
                depth: Some(25),
                frequency: None
            }))
            .unwrap(),
            "orderBook25.H"
        );
    }

    #[test]
    fn unsupported_lob_params_are_rejected() {
        let params = [
            LobParam::Bbo { frequency: None },
            LobParam::Snapshot {
                depth: Some(25),
                frequency: None,
            },
            LobParam::Incremental {
                depth: Some(50),
                frequency: None,
            },
            LobParam::Incremental {
                depth: Some(25),
                frequency: Some(LobFrequency::Ms100),
            },
            LobParam::Incremental {
                depth: None,
                frequency: Some(LobFrequency::Realtime),
            },
        ];

        for param in params {
            assert!(apex_lob_topic(&Some(param.clone())).is_err(), "{param:?}");
        }
    }

    #[test]
    fn trades_are_individual_only() {
        assert_eq!(apex_trades_topic(&None).unwrap(), "recentlyTrade.H");
        assert_eq!(
            apex_trades_topic(&Some(TradesParam::AllTrades)).unwrap(),
            "recentlyTrade.H"
        );
        assert!(apex_trades_topic(&Some(TradesParam::AggTrades)).is_err());
    }

    #[test]
    fn orderbook_limit_is_bounded() {
        assert_eq!(apex_orderbook_limit(0).unwrap(), 200);
        assert_eq!(apex_orderbook_limit(7).unwrap(), 7);
        assert_eq!(apex_orderbook_limit(200).unwrap(), 200);
        assert!(apex_orderbook_limit(201).is_err());
    }
}
