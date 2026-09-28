use tracing::warn;

use extrema_infra::prelude::{InfraError, InfraResult, LobFrequency, LobParam, TradesParam};

use super::config_assets::EXTENDED_WS;

/// `BTC-USD` -> `BTC_USD_PERP`, `NVDA_24_5-USD` -> `NVDA_24_5_USD_PERP`.
pub fn extended_market_to_cli(market: &str) -> String {
    match market.rsplit_once('-') {
        Some((base, quote)) => format!("{base}_{quote}_PERP"),
        None => {
            warn!("Invalid Extended market: {}", market);
            market.to_string()
        },
    }
}

/// `BTCSPOT-USD` -> `BTCSPOT_USD`.
pub fn extended_spot_to_cli(market: &str) -> String {
    match market.rsplit_once('-') {
        Some((base, quote)) => format!("{base}_{quote}"),
        None => {
            warn!("Invalid Extended market: {}", market);
            market.to_string()
        },
    }
}

/// `NVDA_24_5_USD_PERP` -> `NVDA_24_5-USD`; bases may contain `_`, the quote is the last segment.
pub fn cli_perp_to_extended_market(inst: &str) -> InfraResult<String> {
    inst.strip_suffix("_PERP")
        .and_then(|pair| pair.rsplit_once('_'))
        .filter(|(base, quote)| !base.is_empty() && !quote.is_empty())
        .map(|(base, quote)| format!("{base}-{quote}"))
        .ok_or_else(|| {
            InfraError::ApiCliError(format!(
                "Extended instruments are <BASE>_<QUOTE>_PERP, got {inst}"
            ))
        })
}

/// `{EXTENDED_WS}/{stream}[/{market}][?depth={depth}]`; without a market the stream carries every market.
pub fn ws_stream_url_extended(stream: &str, market: Option<&str>, depth: Option<u8>) -> String {
    let mut url = format!("{EXTENDED_WS}/{stream}");
    if let Some(market) = market {
        url.push('/');
        url.push_str(market);
    }
    if let Some(depth) = depth {
        url.push_str(&format!("?depth={depth}"));
    }

    url
}

/// Stream and optional `depth` query for a book stream.
pub fn extended_lob_stream(
    lob_param: &Option<LobParam>,
) -> InfraResult<(&'static str, Option<u8>)> {
    match lob_param {
        None
        | Some(LobParam::Incremental {
            depth: None,
            frequency: None | Some(LobFrequency::Ms100),
        }) => Ok(("orderbooks", None)),
        Some(LobParam::Bbo {
            frequency: None | Some(LobFrequency::Ms10),
        }) => Ok(("orderbooks", Some(1))),
        Some(param) => Err(InfraError::ApiCliError(format!(
            "Extended pushes the full book as a snapshot then 100ms deltas, or a 10ms best bid/ask; unsupported {:?}",
            param
        ))),
    }
}

pub fn extended_trades_stream(trades_param: &Option<TradesParam>) -> InfraResult<&'static str> {
    match trades_param {
        None | Some(TradesParam::AllTrades) => Ok("publicTrades"),
        Some(TradesParam::AggTrades) => Err(InfraError::ApiCliError(
            "Extended publishes individual trades only".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markets_round_trip_through_cli_names() {
        for (market, inst) in [
            ("BTC-USD", "BTC_USD_PERP"),
            ("NVDA_24_5-USD", "NVDA_24_5_USD_PERP"),
            ("1000PEPE-USD", "1000PEPE_USD_PERP"),
            ("PLACE_JPY-USD_1-USD", "PLACE_JPY-USD_1_USD_PERP"),
        ] {
            assert_eq!(extended_market_to_cli(market), inst);
            assert_eq!(cli_perp_to_extended_market(inst).unwrap(), market);
        }
        assert_eq!(extended_spot_to_cli("BTCSPOT-USD"), "BTCSPOT_USD");
    }

    #[test]
    fn malformed_instruments_are_rejected() {
        for inst in [
            "BTC-USD",
            "BTC_USD",
            "BTC_PERP",
            "_USD_PERP",
            "BTC__PERP",
            "BTCSPOT_USD",
        ] {
            assert!(cli_perp_to_extended_market(inst).is_err(), "{inst}");
        }
    }

    #[test]
    fn stream_urls_carry_the_market_and_depth() {
        assert_eq!(
            ws_stream_url_extended("orderbooks", Some("BTC-USD"), Some(1)),
            "wss://api.starknet.extended.exchange/stream.extended.exchange/v1/orderbooks/BTC-USD?depth=1"
        );
        assert_eq!(
            ws_stream_url_extended("publicTrades", Some("NVDA_24_5-USD"), None),
            "wss://api.starknet.extended.exchange/stream.extended.exchange/v1/publicTrades/NVDA_24_5-USD"
        );
        assert_eq!(
            ws_stream_url_extended("prices/mark", None, None),
            "wss://api.starknet.extended.exchange/stream.extended.exchange/v1/prices/mark"
        );
    }

    #[test]
    fn lob_params_map_to_streams() {
        assert_eq!(extended_lob_stream(&None).unwrap(), ("orderbooks", None));
        assert_eq!(
            extended_lob_stream(&Some(LobParam::Incremental {
                depth: None,
                frequency: Some(LobFrequency::Ms100)
            }))
            .unwrap(),
            ("orderbooks", None)
        );
        assert_eq!(
            extended_lob_stream(&Some(LobParam::Bbo { frequency: None })).unwrap(),
            ("orderbooks", Some(1))
        );
        assert_eq!(
            extended_lob_stream(&Some(LobParam::Bbo {
                frequency: Some(LobFrequency::Ms10)
            }))
            .unwrap(),
            ("orderbooks", Some(1))
        );
    }

    #[test]
    fn unsupported_lob_params_are_rejected() {
        let params = [
            LobParam::Snapshot {
                depth: Some(5),
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
                frequency: Some(LobFrequency::Ms250),
            },
            LobParam::Bbo {
                frequency: Some(LobFrequency::Realtime),
            },
        ];

        for param in params {
            assert!(
                extended_lob_stream(&Some(param.clone())).is_err(),
                "{param:?}"
            );
        }
    }

    #[test]
    fn trades_are_individual_only() {
        assert_eq!(extended_trades_stream(&None).unwrap(), "publicTrades");
        assert_eq!(
            extended_trades_stream(&Some(TradesParam::AllTrades)).unwrap(),
            "publicTrades"
        );
        assert!(extended_trades_stream(&Some(TradesParam::AggTrades)).is_err());
    }
}
