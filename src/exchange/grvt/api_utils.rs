use serde_json::json;

use extrema_infra::prelude::{InfraError, InfraResult, LobFrequency, LobParam, TradesParam};

use super::config_assets::GRVT_MAX_BOOK_LEVELS;

const GRVT_TRADE_FILLS: u64 = 1_000_000;
const GRVT_TRADE_SNAPSHOT: &str = "50";

/// `BTC_USDT_Perp` -> `BTC_USDT_PERP`; other kinds are left as is.
pub fn grvt_inst_to_cli(instrument: &str) -> String {
    match instrument.strip_suffix("_Perp") {
        Some(pair) => format!("{pair}_PERP"),
        None => instrument.to_string(),
    }
}

/// `BTC_USDT_PERP` -> `BTC_USDT_Perp`.
pub fn cli_perp_to_grvt_inst(inst: &str) -> InfraResult<String> {
    inst.strip_suffix("_PERP")
        .and_then(|pair| pair.split_once('_'))
        .filter(|(base, quote)| !base.is_empty() && !quote.is_empty() && !quote.contains('_'))
        .map(|(base, quote)| format!("{base}_{quote}_Perp"))
        .ok_or_else(|| {
            InfraError::ApiCliError(format!(
                "GRVT instruments are <BASE>_<QUOTE>_PERP, got {inst}"
            ))
        })
}

/// GRVT times are unix nanoseconds.
pub fn grvt_ns_to_micros(ns: u64) -> u64 {
    ns / 1_000
}

/// `198827910-2` (taker execution, fill) -> `198827910000002`, which keeps the venue's order.
pub fn grvt_trade_id_to_u64(trade_id: &str) -> Option<u64> {
    let (execution, fill) = trade_id.split_once('-')?;
    let execution: u64 = execution.parse().ok()?;
    let fill: u64 = fill.parse().ok().filter(|fill| *fill < GRVT_TRADE_FILLS)?;

    execution.checked_mul(GRVT_TRADE_FILLS)?.checked_add(fill)
}

pub fn ws_subscribe_msg_grvt(stream: &str, selectors: &[String]) -> String {
    json!({
        "jsonrpc": "2.0",
        "method": "subscribe",
        "params": {
            "stream": stream,
            "selectors": selectors,
        },
        "id": 1
    })
    .to_string()
}

/// `BTC_USDT_PERP` and `50` -> `BTC_USDT_Perp@50`, for every instrument.
pub fn grvt_selectors(insts: Option<&[String]>, secondary: &str) -> InfraResult<Vec<String>> {
    let insts = insts.unwrap_or_default();
    if insts.is_empty() {
        return Err(InfraError::ApiCliError(
            "GRVT ws requires at least one instrument".into(),
        ));
    }

    insts
        .iter()
        .map(|inst| Ok(format!("{}@{secondary}", cli_perp_to_grvt_inst(inst)?)))
        .collect()
}

/// Stream and selector suffix for a book subscription.
pub fn grvt_lob_stream(lob_param: &Option<LobParam>) -> InfraResult<(&'static str, String)> {
    match lob_param {
        None => Ok(("v1.book.d", grvt_delta_rate(&None)?.to_string())),
        Some(LobParam::Bbo { frequency }) => {
            let rate = match frequency {
                None => 200,
                Some(LobFrequency::Ms500) => 500,
                Some(LobFrequency::Ms1000) => 1000,
                Some(freq) => {
                    return Err(InfraError::ApiCliError(format!(
                        "GRVT mini ticker snapshots support only 200ms (default), 500ms, or 1000ms: {:?}",
                        freq
                    )));
                },
            };

            Ok(("v1.mini.s", rate.to_string()))
        },
        Some(LobParam::Snapshot { depth, frequency }) => {
            let depth = match depth.as_ref().copied() {
                None => 10,
                Some(depth @ (10 | 50 | 100 | 500)) => depth,
                Some(depth) => {
                    return Err(InfraError::ApiCliError(format!(
                        "GRVT book snapshots support only 10, 50, 100, or 500 levels: {}",
                        depth
                    )));
                },
            };
            let rate = match frequency {
                None | Some(LobFrequency::Ms500) => 500,
                Some(LobFrequency::Ms1000) => 1000,
                Some(freq) => {
                    return Err(InfraError::ApiCliError(format!(
                        "GRVT book snapshots support only 500ms or 1000ms: {:?}",
                        freq
                    )));
                },
            };

            Ok(("v1.book.s", format!("{rate}-{depth}")))
        },
        Some(LobParam::Incremental { depth, frequency }) => {
            if depth.is_some() {
                return Err(InfraError::ApiCliError(format!(
                    "GRVT book deltas always cover the full book: {:?}",
                    depth
                )));
            }

            Ok(("v1.book.d", grvt_delta_rate(frequency)?.to_string()))
        },
    }
}

/// Stream and selector suffix for trades; the suffix is the replayed history, which is not emitted.
pub fn grvt_trades_stream(
    trades_param: &Option<TradesParam>,
) -> InfraResult<(&'static str, String)> {
    match trades_param {
        None | Some(TradesParam::AllTrades) => Ok(("v1.trade", GRVT_TRADE_SNAPSHOT.into())),
        Some(TradesParam::AggTrades) => Err(InfraError::ApiCliError(
            "GRVT publishes individual fills only".into(),
        )),
    }
}

pub(crate) fn grvt_orderbook_depth(depth: usize) -> InfraResult<usize> {
    match depth {
        0 => Ok(GRVT_MAX_BOOK_LEVELS),
        depth @ (10 | 50 | 100 | 500) => Ok(depth),
        depth => Err(InfraError::ApiCliError(format!(
            "GRVT orderbook supports only 10, 50, 100, or 500 levels: {}",
            depth
        ))),
    }
}

fn grvt_delta_rate(frequency: &Option<LobFrequency>) -> InfraResult<u16> {
    match frequency {
        None => Ok(50),
        Some(LobFrequency::Ms100) => Ok(100),
        Some(LobFrequency::Ms500) => Ok(500),
        Some(LobFrequency::Ms1000) => Ok(1000),
        Some(freq) => Err(InfraError::ApiCliError(format!(
            "GRVT book deltas support only 50ms (default), 100ms, 500ms, or 1000ms: {:?}",
            freq
        ))),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn instruments_round_trip_through_cli_names() {
        for (instrument, inst) in [
            ("BTC_USDT_Perp", "BTC_USDT_PERP"),
            ("NVDA_USDT_Perp", "NVDA_USDT_PERP"),
            ("KODEX200_USDT_Perp", "KODEX200_USDT_PERP"),
            ("H_USDT_Perp", "H_USDT_PERP"),
        ] {
            assert_eq!(grvt_inst_to_cli(instrument), inst);
            assert_eq!(cli_perp_to_grvt_inst(inst).unwrap(), instrument);
        }
    }

    #[test]
    fn other_kinds_are_left_as_is() {
        assert_eq!(
            grvt_inst_to_cli("BTC_USDT_Fut_20Oct23"),
            "BTC_USDT_Fut_20Oct23"
        );
    }

    #[test]
    fn malformed_instruments_are_rejected() {
        for inst in [
            "BTC_USDT_Perp",
            "BTC_USDT",
            "BTC_PERP",
            "_USDT_PERP",
            "BTC__PERP",
            "A_B_C_PERP",
            "BTCUSDT",
        ] {
            assert!(cli_perp_to_grvt_inst(inst).is_err(), "{inst}");
        }
    }

    #[test]
    fn nanoseconds_become_micros() {
        assert_eq!(
            grvt_ns_to_micros(1_790_586_064_400_000_000),
            1_790_586_064_400_000
        );
    }

    #[test]
    fn trade_ids_pack_execution_and_fill() {
        assert_eq!(
            grvt_trade_id_to_u64("198827910-1"),
            Some(198_827_910_000_001)
        );
        assert_eq!(
            grvt_trade_id_to_u64("198669999-12"),
            Some(198_669_999_000_012)
        );
        assert!(grvt_trade_id_to_u64("198669999-2") > grvt_trade_id_to_u64("198669999-1"));
        assert!(grvt_trade_id_to_u64("198670014-1") > grvt_trade_id_to_u64("198669999-12"));
    }

    #[test]
    fn malformed_trade_ids_are_rejected() {
        for id in [
            "198827910",
            "trade-1",
            "198827910-",
            "-1",
            "1-2-3",
            "1-1000000",
            "18446744073709551615-1",
        ] {
            assert_eq!(grvt_trade_id_to_u64(id), None, "{id}");
        }
    }

    #[test]
    fn subscribe_msg_is_json_rpc() {
        let msg: Value = serde_json::from_str(&ws_subscribe_msg_grvt(
            "v1.book.d",
            &["BTC_USDT_Perp@50".to_string()],
        ))
        .unwrap();

        assert_eq!(
            msg,
            json!({"jsonrpc":"2.0","method":"subscribe",
                "params":{"stream":"v1.book.d","selectors":["BTC_USDT_Perp@50"]},"id":1})
        );
    }

    #[test]
    fn selectors_cover_every_instrument() {
        let insts = vec!["BTC_USDT_PERP".to_string(), "NVDA_USDT_PERP".to_string()];

        assert_eq!(
            grvt_selectors(Some(&insts), "500-10").unwrap(),
            vec!["BTC_USDT_Perp@500-10", "NVDA_USDT_Perp@500-10"]
        );
        assert!(grvt_selectors(None, "50").is_err());
        assert!(grvt_selectors(Some(&[]), "50").is_err());
        assert!(grvt_selectors(Some(&["BTC-USDT".to_string()]), "50").is_err());
    }

    #[test]
    fn lob_params_map_to_streams() {
        let cases = [
            (None, ("v1.book.d", "50")),
            (
                Some(LobParam::Incremental {
                    depth: None,
                    frequency: Some(LobFrequency::Ms100),
                }),
                ("v1.book.d", "100"),
            ),
            (
                Some(LobParam::Incremental {
                    depth: None,
                    frequency: Some(LobFrequency::Ms1000),
                }),
                ("v1.book.d", "1000"),
            ),
            (
                Some(LobParam::Snapshot {
                    depth: None,
                    frequency: None,
                }),
                ("v1.book.s", "500-10"),
            ),
            (
                Some(LobParam::Snapshot {
                    depth: Some(500),
                    frequency: Some(LobFrequency::Ms1000),
                }),
                ("v1.book.s", "1000-500"),
            ),
            (
                Some(LobParam::Bbo { frequency: None }),
                ("v1.mini.s", "200"),
            ),
            (
                Some(LobParam::Bbo {
                    frequency: Some(LobFrequency::Ms500),
                }),
                ("v1.mini.s", "500"),
            ),
        ];

        for (param, (stream, secondary)) in cases {
            assert_eq!(
                grvt_lob_stream(&param).unwrap(),
                (stream, secondary.to_string()),
                "{param:?}"
            );
        }
    }

    #[test]
    fn unsupported_lob_params_are_rejected() {
        let params = [
            LobParam::Bbo {
                frequency: Some(LobFrequency::Realtime),
            },
            LobParam::Bbo {
                frequency: Some(LobFrequency::Ms100),
            },
            LobParam::Snapshot {
                depth: Some(5),
                frequency: None,
            },
            LobParam::Snapshot {
                depth: Some(10),
                frequency: Some(LobFrequency::Ms100),
            },
            LobParam::Incremental {
                depth: Some(10),
                frequency: None,
            },
            LobParam::Incremental {
                depth: None,
                frequency: Some(LobFrequency::Ms250),
            },
            LobParam::Incremental {
                depth: None,
                frequency: Some(LobFrequency::Realtime),
            },
        ];

        for param in params {
            assert!(grvt_lob_stream(&Some(param.clone())).is_err(), "{param:?}");
        }
    }

    #[test]
    fn trades_are_individual_fills() {
        assert_eq!(
            grvt_trades_stream(&None).unwrap(),
            ("v1.trade", "50".to_string())
        );
        assert_eq!(
            grvt_trades_stream(&Some(TradesParam::AllTrades)).unwrap(),
            ("v1.trade", "50".to_string())
        );
        assert!(grvt_trades_stream(&Some(TradesParam::AggTrades)).is_err());
    }

    #[test]
    fn orderbook_depth_accepts_only_venue_levels() {
        assert_eq!(grvt_orderbook_depth(0).unwrap(), 500);
        assert_eq!(grvt_orderbook_depth(10).unwrap(), 10);
        assert_eq!(grvt_orderbook_depth(100).unwrap(), 100);
        assert!(grvt_orderbook_depth(5).is_err());
        assert!(grvt_orderbook_depth(1000).is_err());
    }
}
