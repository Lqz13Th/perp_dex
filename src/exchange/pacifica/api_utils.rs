use serde_json::{Map, Value, json};

use extrema_infra::prelude::{
    InfraError, InfraResult, InstrumentType, LobFrequency, LobParam, TradesParam,
};

use super::config_assets::{PACIFICA_BOOK_AGG_LEVEL, PACIFICA_MAX_BOOK_LEVELS, PACIFICA_QUOTE};

/// `BTC` -> `BTC_USDC_PERP`, `kBONK` -> `kBONK_USDC_PERP`, spot `SOL-USDC` -> `SOL_USDC`.
///
/// Pacifica symbols are case sensitive, so the base keeps the venue's case.
pub fn pacifica_symbol_to_cli(symbol: &str) -> String {
    match symbol.split_once('-') {
        Some((base, quote)) => format!("{base}_{quote}"),
        None => format!("{symbol}_{PACIFICA_QUOTE}_PERP"),
    }
}

/// `BTC_USDC_PERP` -> `BTC`, `SOL_USDC` -> `SOL-USDC`.
pub fn cli_to_pacifica_symbol(inst: &str) -> InfraResult<String> {
    let symbol = match inst.strip_suffix("_PERP") {
        Some(pair) => pair
            .strip_suffix(PACIFICA_QUOTE)
            .and_then(|base| base.strip_suffix('_'))
            .filter(|base| is_pacifica_asset(base))
            .map(str::to_string),
        None => inst
            .split_once('_')
            .filter(|(base, quote)| is_pacifica_asset(base) && is_pacifica_asset(quote))
            .map(|(base, quote)| format!("{base}-{quote}")),
    };

    symbol.ok_or_else(|| {
        InfraError::ApiCliError(format!(
            "Pacifica instruments are <BASE>_{PACIFICA_QUOTE}_PERP or spot <BASE>_<QUOTE>, got {inst}"
        ))
    })
}

/// Spot markets are the only symbols with a `-`.
pub fn pacifica_inst_type(symbol: &str) -> InstrumentType {
    if symbol.contains('-') {
        InstrumentType::Spot
    } else {
        InstrumentType::Perpetual
    }
}

fn is_pacifica_asset(asset: &str) -> bool {
    !asset.is_empty() && !asset.contains(['_', '-'])
}

pub fn ws_subscribe_msg_pacifica(
    source: &str,
    symbol: Option<&str>,
    agg_level: Option<u16>,
) -> String {
    let mut params = Map::new();
    params.insert("source".into(), json!(source));
    if let Some(symbol) = symbol {
        params.insert("symbol".into(), json!(symbol));
    }
    if let Some(agg_level) = agg_level {
        params.insert("agg_level".into(), json!(agg_level));
    }

    json!({
        "method": "subscribe",
        "params": Value::Object(params),
    })
    .to_string()
}

/// Source and `agg_level` for a book subscription; `book` needs an `agg_level`.
pub fn pacifica_lob_source(
    lob_param: &Option<LobParam>,
) -> InfraResult<(&'static str, Option<u16>)> {
    match lob_param {
        None
        | Some(LobParam::Snapshot {
            depth: None | Some(10),
            frequency: None | Some(LobFrequency::Ms250),
        }) => Ok(("book", Some(PACIFICA_BOOK_AGG_LEVEL))),
        Some(LobParam::Bbo {
            frequency: None | Some(LobFrequency::Realtime),
        }) => Ok(("bbo", None)),
        Some(param) => Err(InfraError::ApiCliError(format!(
            "Pacifica pushes a full {PACIFICA_MAX_BOOK_LEVELS}-level book every 250ms, or a realtime bbo; unsupported {:?}",
            param
        ))),
    }
}

pub fn pacifica_trades_source(trades_param: &Option<TradesParam>) -> InfraResult<&'static str> {
    match trades_param {
        None | Some(TradesParam::AllTrades) => Ok("trades"),
        Some(TradesParam::AggTrades) => Err(InfraError::ApiCliError(
            "Pacifica publishes individual fills only".into(),
        )),
    }
}

pub(crate) fn pacifica_orderbook_levels(depth: usize) -> InfraResult<usize> {
    match depth {
        0 => Ok(PACIFICA_MAX_BOOK_LEVELS),
        1..=PACIFICA_MAX_BOOK_LEVELS => Ok(depth),
        depth => Err(InfraError::ApiCliError(format!(
            "Pacifica orderbook supports 1 to {PACIFICA_MAX_BOOK_LEVELS} levels: {depth}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbols_round_trip_through_cli_names() {
        for (symbol, inst) in [
            ("BTC", "BTC_USDC_PERP"),
            ("NVDA", "NVDA_USDC_PERP"),
            ("kBONK", "kBONK_USDC_PERP"),
            ("2Z", "2Z_USDC_PERP"),
            ("USDJPY", "USDJPY_USDC_PERP"),
            ("SOL-USDC", "SOL_USDC"),
        ] {
            assert_eq!(pacifica_symbol_to_cli(symbol), inst);
            assert_eq!(cli_to_pacifica_symbol(inst).unwrap(), symbol);
        }
    }

    #[test]
    fn malformed_or_foreign_instruments_are_rejected() {
        for inst in [
            "BTC",
            "BTC-USDC",
            "BTC_PERP",
            "_USDC_PERP",
            "BTC_USDT_PERP",
            "BTCUSDC_PERP",
            "A_B_USDC_PERP",
            "SOL_",
            "_USDC",
            "A_B_C",
            "@1",
        ] {
            assert!(cli_to_pacifica_symbol(inst).is_err(), "{inst}");
        }
    }

    #[test]
    fn instrument_type_follows_the_symbol_shape() {
        assert_eq!(pacifica_inst_type("BTC"), InstrumentType::Perpetual);
        assert_eq!(pacifica_inst_type("kPEPE"), InstrumentType::Perpetual);
        assert_eq!(pacifica_inst_type("SOL-USDC"), InstrumentType::Spot);
    }

    #[test]
    fn subscribe_msg_carries_only_given_fields() {
        let msg: Value =
            serde_json::from_str(&ws_subscribe_msg_pacifica("book", Some("BTC"), Some(1))).unwrap();
        assert_eq!(
            msg,
            json!({"method":"subscribe","params":{"source":"book","symbol":"BTC","agg_level":1}})
        );

        let msg: Value =
            serde_json::from_str(&ws_subscribe_msg_pacifica("prices", None, None)).unwrap();
        assert_eq!(
            msg,
            json!({"method":"subscribe","params":{"source":"prices"}})
        );
    }

    #[test]
    fn lob_params_map_to_sources() {
        let book = ("book", Some(1));

        assert_eq!(pacifica_lob_source(&None).unwrap(), book);
        assert_eq!(
            pacifica_lob_source(&Some(LobParam::Snapshot {
                depth: None,
                frequency: None
            }))
            .unwrap(),
            book
        );
        assert_eq!(
            pacifica_lob_source(&Some(LobParam::Snapshot {
                depth: Some(10),
                frequency: Some(LobFrequency::Ms250)
            }))
            .unwrap(),
            book
        );
        assert_eq!(
            pacifica_lob_source(&Some(LobParam::Bbo { frequency: None })).unwrap(),
            ("bbo", None)
        );
        assert_eq!(
            pacifica_lob_source(&Some(LobParam::Bbo {
                frequency: Some(LobFrequency::Realtime)
            }))
            .unwrap(),
            ("bbo", None)
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
                frequency: Some(LobFrequency::Ms100),
            },
            LobParam::Incremental {
                depth: None,
                frequency: None,
            },
            LobParam::Bbo {
                frequency: Some(LobFrequency::Ms100),
            },
        ];

        for param in params {
            assert!(
                pacifica_lob_source(&Some(param.clone())).is_err(),
                "{param:?}"
            );
        }
    }

    #[test]
    fn trades_are_individual_only() {
        assert_eq!(pacifica_trades_source(&None).unwrap(), "trades");
        assert_eq!(
            pacifica_trades_source(&Some(TradesParam::AllTrades)).unwrap(),
            "trades"
        );
        assert!(pacifica_trades_source(&Some(TradesParam::AggTrades)).is_err());
    }

    #[test]
    fn orderbook_levels_are_bounded() {
        assert_eq!(pacifica_orderbook_levels(0).unwrap(), 10);
        assert_eq!(pacifica_orderbook_levels(5).unwrap(), 5);
        assert!(pacifica_orderbook_levels(11).is_err());
    }
}
