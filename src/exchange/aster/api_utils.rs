use serde_json::json;
use tracing::warn;

use extrema_infra::prelude::{InfraError, InfraResult, LobFrequency, LobParam, TradesParam};

const ASTER_QUOTES: [&str; 5] = ["USDT", "USDC", "USD1", "USD", "U"];

/// `NVDAUSDT` -> `NVDA_USDT_PERP`.
pub fn aster_inst_to_cli(symbol: &str) -> String {
    let upper = symbol.to_uppercase();

    for quote in ASTER_QUOTES {
        if let Some(base) = upper.strip_suffix(quote) {
            if base.is_empty() {
                warn!("Invalid Aster symbol: {}", symbol);
                return upper;
            }
            return format!("{}_{}_PERP", base, quote);
        }
    }

    upper
}

/// `NVDA_USDT_PERP` -> `NVDAUSDT`.
pub fn cli_perp_to_aster_symbol(inst: &str) -> String {
    inst.strip_suffix("_PERP")
        .unwrap_or(inst)
        .replace('_', "")
        .to_uppercase()
}

pub fn ws_subscribe_msg_aster(stream: &str, insts: Option<&[String]>) -> String {
    let params: Vec<String> = match insts {
        Some(list) => list
            .iter()
            .map(|inst| {
                format!(
                    "{}@{}",
                    cli_perp_to_aster_symbol(inst).to_lowercase(),
                    stream
                )
            })
            .collect(),
        None => vec![stream.into()],
    };

    json!({
        "method": "SUBSCRIBE",
        "params": params,
        "id": 1
    })
    .to_string()
}

pub fn aster_lob_stream(lob_param: &Option<LobParam>) -> InfraResult<String> {
    match lob_param {
        None => Ok(format!("depth{}", aster_lob_frequency_suffix(&None)?)),
        Some(LobParam::Bbo { frequency }) => match frequency {
            None | Some(LobFrequency::Realtime) => Ok("bookTicker".into()),
            Some(freq) => Err(InfraError::ApiCliError(format!(
                "Aster bookTicker does not support requested frequency: {:?}",
                freq
            ))),
        },
        Some(LobParam::Snapshot { depth, frequency }) => {
            let depth = match depth.as_ref().copied() {
                None => 20,
                Some(depth @ (5 | 10 | 20)) => depth,
                Some(depth) => {
                    return Err(InfraError::ApiCliError(format!(
                        "Aster partial depth supports only 5, 10, or 20 levels: {}",
                        depth
                    )));
                },
            };

            Ok(format!(
                "depth{}{}",
                depth,
                aster_lob_frequency_suffix(frequency)?
            ))
        },
        Some(LobParam::Incremental { depth, frequency }) => {
            if depth.is_some() {
                return Err(InfraError::ApiCliError(format!(
                    "Aster diff depth stream does not take a depth: {:?}",
                    depth
                )));
            }

            Ok(format!("depth{}", aster_lob_frequency_suffix(frequency)?))
        },
    }
}

pub fn aster_trades_stream(trades_param: &Option<TradesParam>) -> &'static str {
    match trades_param {
        Some(TradesParam::AllTrades) => "trade",
        None | Some(TradesParam::AggTrades) => "aggTrade",
    }
}

pub(crate) fn aster_orderbook_limit(depth: usize) -> InfraResult<usize> {
    match depth {
        0 => Ok(500),
        depth @ (5 | 10 | 20 | 50 | 100 | 500 | 1000) => Ok(depth),
        depth => Err(InfraError::ApiCliError(format!(
            "Aster orderbook supports only 5, 10, 20, 50, 100, 500, or 1000 levels: {}",
            depth
        ))),
    }
}

fn aster_lob_frequency_suffix(frequency: &Option<LobFrequency>) -> InfraResult<&'static str> {
    match frequency {
        None | Some(LobFrequency::Ms250) => Ok(""),
        Some(LobFrequency::Ms100) => Ok("@100ms"),
        Some(LobFrequency::Ms500) => Ok("@500ms"),
        Some(freq) => Err(InfraError::ApiCliError(format!(
            "Aster LOB supports only 100ms, 250ms, or 500ms frequency: {:?}",
            freq
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_round_trip_through_cli_names() {
        let cases = [
            ("NVDAUSDT", "NVDA_USDT_PERP"),
            ("MUUSD1", "MU_USD1_PERP"),
            ("GNSUSD", "GNS_USD_PERP"),
            ("BTCU", "BTC_U_PERP"),
            ("1000PEPEUSDT", "1000PEPE_USDT_PERP"),
            ("nvdausdt", "NVDA_USDT_PERP"),
        ];

        for (symbol, inst) in cases {
            assert_eq!(aster_inst_to_cli(symbol), inst);
            assert_eq!(cli_perp_to_aster_symbol(inst), symbol.to_uppercase());
        }
    }

    #[test]
    fn unknown_quote_is_left_as_is() {
        assert_eq!(aster_inst_to_cli("USDT"), "USDT");
        assert_eq!(aster_inst_to_cli("ABCXYZ"), "ABCXYZ");
    }

    #[test]
    fn subscribe_msg_lists_every_instrument() {
        let insts = vec!["NVDA_USDT_PERP".to_string(), "MU_USD1_PERP".to_string()];
        let msg: serde_json::Value =
            serde_json::from_str(&ws_subscribe_msg_aster("bookTicker", Some(&insts))).unwrap();

        assert_eq!(msg["method"], "SUBSCRIBE");
        assert_eq!(
            msg["params"],
            json!(["nvdausdt@bookTicker", "muusd1@bookTicker"])
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&ws_subscribe_msg_aster(
                "!markPrice@arr",
                None
            ))
            .unwrap()["params"],
            json!(["!markPrice@arr"])
        );
    }

    #[test]
    fn lob_streams_follow_the_param() {
        assert_eq!(aster_lob_stream(&None).unwrap(), "depth");
        assert_eq!(
            aster_lob_stream(&Some(LobParam::Bbo { frequency: None })).unwrap(),
            "bookTicker"
        );
        assert_eq!(
            aster_lob_stream(&Some(LobParam::Snapshot {
                depth: None,
                frequency: Some(LobFrequency::Ms100)
            }))
            .unwrap(),
            "depth20@100ms"
        );
        assert_eq!(
            aster_lob_stream(&Some(LobParam::Snapshot {
                depth: Some(5),
                frequency: Some(LobFrequency::Ms500)
            }))
            .unwrap(),
            "depth5@500ms"
        );
        assert_eq!(
            aster_lob_stream(&Some(LobParam::Incremental {
                depth: None,
                frequency: Some(LobFrequency::Ms100)
            }))
            .unwrap(),
            "depth@100ms"
        );
    }

    #[test]
    fn lob_streams_reject_unsupported_params() {
        assert!(
            aster_lob_stream(&Some(LobParam::Bbo {
                frequency: Some(LobFrequency::Ms100)
            }))
            .is_err()
        );
        assert!(
            aster_lob_stream(&Some(LobParam::Snapshot {
                depth: Some(50),
                frequency: None
            }))
            .is_err()
        );
        assert!(
            aster_lob_stream(&Some(LobParam::Incremental {
                depth: Some(20),
                frequency: None
            }))
            .is_err()
        );
        assert!(
            aster_lob_stream(&Some(LobParam::Incremental {
                depth: None,
                frequency: Some(LobFrequency::Ms10)
            }))
            .is_err()
        );
    }

    #[test]
    fn trades_stream_defaults_to_aggregated() {
        assert_eq!(aster_trades_stream(&None), "aggTrade");
        assert_eq!(
            aster_trades_stream(&Some(TradesParam::AggTrades)),
            "aggTrade"
        );
        assert_eq!(aster_trades_stream(&Some(TradesParam::AllTrades)), "trade");
    }

    #[test]
    fn orderbook_limit_accepts_only_venue_levels() {
        assert_eq!(aster_orderbook_limit(0).unwrap(), 500);
        assert_eq!(aster_orderbook_limit(20).unwrap(), 20);
        assert!(aster_orderbook_limit(7).is_err());
    }
}
