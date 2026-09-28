use serde_json::{Map, Value, json};

use extrema_infra::prelude::{InfraError, InfraResult, LobParam, TradesParam};

use super::config_assets::ARCUS_MAX_BOOK_LEVELS;

/// `NVDA-USD` -> `NVDA_USD_PERP`.
pub fn arcus_market_to_cli(market: &str) -> String {
    format!("{}_PERP", market.to_uppercase().replace('-', "_"))
}

/// `NVDA_USD_PERP` -> `NVDA-USD`.
pub fn cli_perp_to_arcus_market(inst: &str) -> InfraResult<String> {
    inst.strip_suffix("_PERP")
        .and_then(|pair| pair.split_once('_'))
        .filter(|(base, quote)| !base.is_empty() && !quote.is_empty() && !quote.contains('_'))
        .map(|(base, quote)| format!("{base}-{quote}"))
        .ok_or_else(|| {
            InfraError::ApiCliError(format!(
                "Arcus instruments are <BASE>_<QUOTE>_PERP, got {inst}"
            ))
        })
}

pub fn ws_subscribe_msg_arcus(
    channel: &str,
    market: Option<&str>,
    n_levels: Option<u16>,
) -> String {
    let mut msg = Map::new();
    msg.insert("type".into(), json!("subscribe"));
    msg.insert("channel".into(), json!(channel));
    if let Some(market) = market {
        msg.insert("id".into(), json!(market));
    }
    if let Some(n_levels) = n_levels {
        msg.insert("nLevels".into(), json!(n_levels));
    }

    Value::Object(msg).to_string()
}

/// Channel and optional `nLevels` for a book subscription.
pub fn arcus_lob_channel(lob_param: &Option<LobParam>) -> InfraResult<(&'static str, Option<u16>)> {
    match lob_param {
        None => Ok(("l2OrderbookUpdates", None)),
        Some(LobParam::Bbo { frequency: None }) => Ok(("bbo", None)),
        Some(LobParam::Snapshot {
            depth,
            frequency: None,
        }) => Ok(("l2Orderbook", arcus_n_levels(*depth)?)),
        Some(LobParam::Incremental {
            depth,
            frequency: None,
        }) => Ok(("l2OrderbookUpdates", arcus_n_levels(*depth)?)),
        Some(param) => Err(InfraError::ApiCliError(format!(
            "Arcus book streams have a fixed cadence; unsupported {:?}",
            param
        ))),
    }
}

pub fn arcus_trades_channel(trades_param: &Option<TradesParam>) -> InfraResult<&'static str> {
    match trades_param {
        None | Some(TradesParam::AllTrades) => Ok("trades"),
        Some(TradesParam::AggTrades) => Err(InfraError::ApiCliError(
            "Arcus publishes individual fills only".into(),
        )),
    }
}

pub(crate) fn arcus_orderbook_levels(depth: usize) -> InfraResult<usize> {
    match depth {
        0 => Ok(ARCUS_MAX_BOOK_LEVELS),
        1..=ARCUS_MAX_BOOK_LEVELS => Ok(depth),
        depth => Err(InfraError::ApiCliError(format!(
            "Arcus orderbook supports 1 to {ARCUS_MAX_BOOK_LEVELS} levels: {depth}"
        ))),
    }
}

fn arcus_n_levels(depth: Option<u16>) -> InfraResult<Option<u16>> {
    match depth {
        None => Ok(None),
        Some(depth) if (1..=ARCUS_MAX_BOOK_LEVELS as u16).contains(&depth) => Ok(Some(depth)),
        Some(depth) => Err(InfraError::ApiCliError(format!(
            "Arcus book streams support 1 to {ARCUS_MAX_BOOK_LEVELS} levels: {depth}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use extrema_infra::prelude::LobFrequency;

    use super::*;

    #[test]
    fn markets_round_trip_through_cli_names() {
        for (market, inst) in [
            ("NVDA-USD", "NVDA_USD_PERP"),
            ("BTC-USD", "BTC_USD_PERP"),
            ("KBONK-USD", "KBONK_USD_PERP"),
        ] {
            assert_eq!(arcus_market_to_cli(market), inst);
            assert_eq!(cli_perp_to_arcus_market(inst).unwrap(), market);
        }
    }

    #[test]
    fn malformed_instruments_are_rejected() {
        for inst in [
            "NVDA-USD",
            "NVDA_USD",
            "NVDA_PERP",
            "_USD_PERP",
            "NVDA__PERP",
            "A_B_C_PERP",
        ] {
            assert!(cli_perp_to_arcus_market(inst).is_err(), "{inst}");
        }
    }

    #[test]
    fn subscribe_msg_carries_only_given_fields() {
        let msg: Value = serde_json::from_str(&ws_subscribe_msg_arcus(
            "l2Orderbook",
            Some("NVDA-USD"),
            Some(5),
        ))
        .unwrap();
        assert_eq!(
            msg,
            json!({"type":"subscribe","channel":"l2Orderbook","id":"NVDA-USD","nLevels":5})
        );

        let msg: Value =
            serde_json::from_str(&ws_subscribe_msg_arcus("markets", None, None)).unwrap();
        assert_eq!(msg, json!({"type":"subscribe","channel":"markets"}));
    }

    #[test]
    fn lob_params_map_to_channels() {
        assert_eq!(
            arcus_lob_channel(&None).unwrap(),
            ("l2OrderbookUpdates", None)
        );
        assert_eq!(
            arcus_lob_channel(&Some(LobParam::Bbo { frequency: None })).unwrap(),
            ("bbo", None)
        );
        assert_eq!(
            arcus_lob_channel(&Some(LobParam::Snapshot {
                depth: Some(5),
                frequency: None
            }))
            .unwrap(),
            ("l2Orderbook", Some(5))
        );
        assert_eq!(
            arcus_lob_channel(&Some(LobParam::Incremental {
                depth: Some(100),
                frequency: None
            }))
            .unwrap(),
            ("l2OrderbookUpdates", Some(100))
        );
    }

    #[test]
    fn unsupported_lob_params_are_rejected() {
        let params = [
            LobParam::Bbo {
                frequency: Some(LobFrequency::Realtime),
            },
            LobParam::Snapshot {
                depth: Some(101),
                frequency: None,
            },
            LobParam::Incremental {
                depth: Some(0),
                frequency: None,
            },
            LobParam::Snapshot {
                depth: None,
                frequency: Some(LobFrequency::Ms100),
            },
        ];

        for param in params {
            assert!(
                arcus_lob_channel(&Some(param.clone())).is_err(),
                "{param:?}"
            );
        }
    }

    #[test]
    fn trades_are_individual_only() {
        assert_eq!(arcus_trades_channel(&None).unwrap(), "trades");
        assert!(arcus_trades_channel(&Some(TradesParam::AggTrades)).is_err());
    }

    #[test]
    fn orderbook_levels_are_bounded() {
        assert_eq!(arcus_orderbook_levels(0).unwrap(), 100);
        assert_eq!(arcus_orderbook_levels(20).unwrap(), 20);
        assert!(arcus_orderbook_levels(101).is_err());
    }
}
