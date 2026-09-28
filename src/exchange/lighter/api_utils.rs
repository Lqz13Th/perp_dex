use serde_json::json;

use extrema_infra::prelude::{InfraError, InfraResult, LobFrequency, LobParam, TradesParam};

/// Lighter frames carry only the market index, so every Lighter instrument is `@<market_id>`.
pub fn lighter_market_to_cli(market_id: u16) -> String {
    format!("@{market_id}")
}

pub fn cli_to_lighter_market_id(inst: &str) -> InfraResult<u16> {
    inst.strip_prefix('@')
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| {
            InfraError::ApiCliError(format!("Lighter instruments are @<market_id>, got {inst}"))
        })
}

/// `order_book:110` -> `@110`.
pub fn lighter_channel_to_cli(channel: &str) -> String {
    let market_id = channel.rsplit_once(':').map_or(channel, |(_, id)| id);
    format!("@{market_id}")
}

pub fn ws_subscribe_msg_lighter(channel: &str, market_id: Option<u16>) -> String {
    let channel = match market_id {
        Some(market_id) => format!("{channel}/{market_id}"),
        None => channel.to_string(),
    };

    json!({
        "type": "subscribe",
        "channel": channel,
    })
    .to_string()
}

pub fn lighter_lob_channel(lob_param: &Option<LobParam>) -> InfraResult<&'static str> {
    match lob_param {
        None
        | Some(LobParam::Incremental {
            depth: None,
            frequency: None,
        }) => Ok("order_book"),
        Some(LobParam::Bbo {
            frequency: None | Some(LobFrequency::Realtime),
        }) => Ok("ticker"),
        Some(param) => Err(InfraError::ApiCliError(format!(
            "Lighter pushes a full book then 50ms incremental batches, or a realtime ticker; unsupported {:?}",
            param
        ))),
    }
}

pub fn lighter_trades_channel(trades_param: &Option<TradesParam>) -> InfraResult<&'static str> {
    match trades_param {
        None | Some(TradesParam::AllTrades) => Ok("trade"),
        Some(TradesParam::AggTrades) => Err(InfraError::ApiCliError(
            "Lighter publishes individual trades only".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn market_ids_round_trip_through_cli_names() {
        assert_eq!(lighter_market_to_cli(110), "@110");
        assert_eq!(cli_to_lighter_market_id("@110").unwrap(), 110);
        assert_eq!(cli_to_lighter_market_id("@0").unwrap(), 0);
        assert_eq!(lighter_channel_to_cli("order_book:110"), "@110");
        assert_eq!(lighter_channel_to_cli("trade:2048"), "@2048");
    }

    #[test]
    fn non_index_instruments_are_rejected() {
        for inst in ["NVDA", "NVDA_USDC_PERP", "@", "@-1", "@70000", "110"] {
            assert!(cli_to_lighter_market_id(inst).is_err(), "{inst}");
        }
    }

    #[test]
    fn subscribe_msg_appends_the_market() {
        let msg: Value =
            serde_json::from_str(&ws_subscribe_msg_lighter("order_book", Some(110))).unwrap();
        assert_eq!(msg["type"], "subscribe");
        assert_eq!(msg["channel"], "order_book/110");

        let msg: Value =
            serde_json::from_str(&ws_subscribe_msg_lighter("market_stats/all", None)).unwrap();
        assert_eq!(msg["channel"], "market_stats/all");
    }

    #[test]
    fn lob_params_map_to_channels() {
        assert_eq!(lighter_lob_channel(&None).unwrap(), "order_book");
        assert_eq!(
            lighter_lob_channel(&Some(LobParam::Incremental {
                depth: None,
                frequency: None
            }))
            .unwrap(),
            "order_book"
        );
        assert_eq!(
            lighter_lob_channel(&Some(LobParam::Bbo { frequency: None })).unwrap(),
            "ticker"
        );
        assert_eq!(
            lighter_lob_channel(&Some(LobParam::Bbo {
                frequency: Some(LobFrequency::Realtime)
            }))
            .unwrap(),
            "ticker"
        );
    }

    #[test]
    fn unsupported_lob_params_are_rejected() {
        let params = [
            LobParam::Snapshot {
                depth: Some(20),
                frequency: None,
            },
            LobParam::Incremental {
                depth: Some(20),
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
            assert!(
                lighter_lob_channel(&Some(param.clone())).is_err(),
                "{param:?}"
            );
        }
    }

    #[test]
    fn trades_are_individual_only() {
        assert_eq!(lighter_trades_channel(&None).unwrap(), "trade");
        assert_eq!(
            lighter_trades_channel(&Some(TradesParam::AllTrades)).unwrap(),
            "trade"
        );
        assert!(lighter_trades_channel(&Some(TradesParam::AggTrades)).is_err());
    }
}
