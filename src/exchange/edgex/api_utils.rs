use serde_json::json;

use extrema_infra::prelude::{InfraError, InfraResult, LobFrequency, LobParam, TradesParam};

use super::config_assets::{EDGEX_BOOK_LEVELS, EDGEX_MAX_BOOK_LEVELS, EDGEX_WS_BBO_ALL};

/// edgeX subscriptions and REST quotes take only the numeric `contractId`, so every
/// edgeX instrument is `@<contract_id>`; `inst_code` carries the contract name.
pub fn edgex_contract_to_cli(contract_id: u64) -> String {
    format!("@{contract_id}")
}

/// `{"type":"pong"}` carrying the client time; edgeX takes it as the reply to its pings.
pub fn ws_pong_msg_edgex(now_ms: u64) -> String {
    json!({
        "type": "pong",
        "time": now_ms.to_string(),
    })
    .to_string()
}

pub fn cli_to_edgex_contract_id(inst: &str) -> InfraResult<u64> {
    inst.strip_prefix('@')
        .filter(|id| !id.starts_with('+'))
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| {
            InfraError::ApiCliError(format!("edgeX instruments are @<contract_id>, got {inst}"))
        })
}

pub fn ws_subscribe_msg_edgex(channel: &str) -> String {
    json!({
        "type": "subscribe",
        "channel": channel,
    })
    .to_string()
}

/// Level of the `depth.{id}.{level}` channel: a full book on subscribe, then deltas.
pub fn edgex_depth_level(lob_param: &Option<LobParam>) -> InfraResult<u16> {
    match lob_param {
        None
        | Some(LobParam::Incremental {
            depth: None,
            frequency: None,
        }) => Ok(EDGEX_MAX_BOOK_LEVELS),
        Some(LobParam::Incremental {
            depth: Some(depth),
            frequency: None,
        }) if EDGEX_BOOK_LEVELS.contains(depth) => Ok(*depth),
        Some(param) => Err(InfraError::ApiCliError(format!(
            "edgeX pushes a full book then deltas at 15 or 200 levels on a fixed cadence; unsupported {:?}",
            param
        ))),
    }
}

pub fn edgex_bbo_channel(frequency: &Option<LobFrequency>) -> InfraResult<&'static str> {
    match frequency {
        None | Some(LobFrequency::Ms1000) => Ok(EDGEX_WS_BBO_ALL),
        Some(freq) => Err(InfraError::ApiCliError(format!(
            "edgeX publishes BBO once a second; unsupported {:?}",
            freq
        ))),
    }
}

pub fn edgex_trades_channel(trades_param: &Option<TradesParam>) -> InfraResult<&'static str> {
    match trades_param {
        None | Some(TradesParam::AllTrades) => Ok("trades"),
        Some(TradesParam::AggTrades) => Err(InfraError::ApiCliError(
            "edgeX publishes individual fills only".into(),
        )),
    }
}

pub(crate) fn edgex_orderbook_level(depth: usize) -> InfraResult<u16> {
    match depth {
        0 => Ok(EDGEX_MAX_BOOK_LEVELS),
        depth
            if EDGEX_BOOK_LEVELS
                .iter()
                .any(|level| *level as usize == depth) =>
        {
            Ok(depth as u16)
        },
        depth => Err(InfraError::ApiCliError(format!(
            "edgeX orderbook supports only 15 or 200 levels: {depth}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn pong_carries_the_client_time() {
        assert_eq!(
            serde_json::from_str::<Value>(&ws_pong_msg_edgex(1790585693937)).unwrap(),
            json!({"type":"pong","time":"1790585693937"})
        );
    }

    #[test]
    fn contract_ids_round_trip_through_cli_names() {
        for id in [30000001, 30000020, 30000182, 0] {
            let inst = edgex_contract_to_cli(id);
            assert_eq!(cli_to_edgex_contract_id(&inst).unwrap(), id);
        }
        assert_eq!(edgex_contract_to_cli(30000020), "@30000020");
    }

    #[test]
    fn non_contract_instruments_are_rejected() {
        for inst in [
            "NVDAUSDC",
            "NVDA_USDC_PERP",
            "30000020",
            "@",
            "@-1",
            "@+30000020",
            "@30000020 ",
            "@nvda",
        ] {
            assert!(cli_to_edgex_contract_id(inst).is_err(), "{inst}");
        }
    }

    #[test]
    fn subscribe_msg_names_the_channel() {
        let msg: Value =
            serde_json::from_str(&ws_subscribe_msg_edgex("depth.30000001.15")).unwrap();

        assert_eq!(
            msg,
            json!({"type":"subscribe","channel":"depth.30000001.15"})
        );
    }

    #[test]
    fn lob_params_map_to_depth_levels() {
        assert_eq!(edgex_depth_level(&None).unwrap(), 200);
        for (depth, level) in [(None, 200), (Some(15), 15), (Some(200), 200)] {
            assert_eq!(
                edgex_depth_level(&Some(LobParam::Incremental {
                    depth,
                    frequency: None
                }))
                .unwrap(),
                level
            );
        }
    }

    #[test]
    fn unsupported_lob_params_are_rejected() {
        let params = [
            LobParam::Snapshot {
                depth: Some(15),
                frequency: None,
            },
            LobParam::Snapshot {
                depth: None,
                frequency: None,
            },
            LobParam::Incremental {
                depth: Some(20),
                frequency: None,
            },
            LobParam::Incremental {
                depth: None,
                frequency: Some(LobFrequency::Realtime),
            },
            LobParam::Bbo { frequency: None },
        ];

        for param in params {
            assert!(
                edgex_depth_level(&Some(param.clone())).is_err(),
                "{param:?}"
            );
        }
    }

    #[test]
    fn bbo_is_once_a_second() {
        assert_eq!(edgex_bbo_channel(&None).unwrap(), "bookTicker.all.1s");
        assert_eq!(
            edgex_bbo_channel(&Some(LobFrequency::Ms1000)).unwrap(),
            "bookTicker.all.1s"
        );
        for freq in [LobFrequency::Realtime, LobFrequency::Ms100] {
            assert!(edgex_bbo_channel(&Some(freq)).is_err());
        }
    }

    #[test]
    fn trades_are_individual_only() {
        assert_eq!(edgex_trades_channel(&None).unwrap(), "trades");
        assert_eq!(
            edgex_trades_channel(&Some(TradesParam::AllTrades)).unwrap(),
            "trades"
        );
        assert!(edgex_trades_channel(&Some(TradesParam::AggTrades)).is_err());
    }

    #[test]
    fn orderbook_levels_are_the_venue_depths() {
        assert_eq!(edgex_orderbook_level(0).unwrap(), 200);
        assert_eq!(edgex_orderbook_level(15).unwrap(), 15);
        assert_eq!(edgex_orderbook_level(200).unwrap(), 200);
        for depth in [1, 5, 14, 16, 100, 201] {
            assert!(edgex_orderbook_level(depth).is_err(), "{depth}");
        }
    }
}
