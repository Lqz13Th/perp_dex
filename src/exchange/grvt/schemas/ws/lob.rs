use serde::Deserialize;

use extrema_infra::{
    arch::market_assets::api_general::de_u64_from_string_or_number,
    prelude::{IntoWsData, LobEventKind, LobLevel, LobLevelAction, LobSeq, WsLob},
};

use crate::exchange::grvt::{
    api_utils::{grvt_inst_to_cli, grvt_ns_to_micros},
    config_assets::GRVT,
};

/// `v1.book.d` sends the full book on subscribe, then every `rate` ms the
/// levels that changed, with absolute sizes. `v1.book.s` frames are all full books.
///
/// `seq.last` is the gateway's per-stream sequence and `seq.prev` the one before
/// it; the subscribe snapshot has neither. The first delta may start anywhere;
/// after that each delta's `seq.prev` must equal the previous `seq.last`, or the
/// book has to be resubscribed. A repeated `seq.last` is a network retry. The
/// sequence restarts when the gateway does.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsBookGrvt {
    stream: GrvtBookStream,
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    sequence_number: u64,
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    prev_sequence_number: u64,
    feed: GrvtBook,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
enum GrvtBookStream {
    #[serde(rename = "v1.book.s")]
    Snapshot,
    #[serde(rename = "v1.book.d")]
    Delta,
}

#[derive(Clone, Debug, Deserialize)]
struct GrvtBook {
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    event_time: u64,
    instrument: String,
    bids: Vec<GrvtLevel>,
    asks: Vec<GrvtLevel>,
}

#[derive(Clone, Debug, Deserialize)]
struct GrvtLevel {
    price: String,
    size: String,
    num_orders: u64,
}

/// `v1.mini.s`: the whole top of book, at most every `rate` ms and only when it
/// changed. An empty side has price 0.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsMiniTickerGrvt {
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    sequence_number: u64,
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    prev_sequence_number: u64,
    feed: GrvtMiniTicker,
}

#[derive(Clone, Debug, Deserialize)]
struct GrvtMiniTicker {
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    event_time: u64,
    instrument: String,
    best_bid_price: String,
    best_bid_size: String,
    best_ask_price: String,
    best_ask_size: String,
}

fn grvt_lob_level(
    price: &str,
    size: &str,
    order_count: Option<u64>,
    delete_on_zero: bool,
) -> LobLevel {
    let size = size.parse().unwrap_or_default();

    LobLevel {
        price: price.parse().unwrap_or_default(),
        size,
        action: if delete_on_zero && size == 0.0 {
            LobLevelAction::Delete
        } else {
            LobLevelAction::Upsert
        },
        order_count,
        level_update_id: None,
    }
}

fn grvt_lob_seq(sequence_number: u64, prev_sequence_number: u64) -> LobSeq {
    LobSeq {
        prev: (prev_sequence_number > 0).then_some(prev_sequence_number),
        first: None,
        last: (sequence_number > 0).then_some(sequence_number),
    }
}

impl IntoWsData for WsBookGrvt {
    type Output = WsLob;

    fn into_ws(self) -> WsLob {
        let book = self.feed;
        let event = if self.stream == GrvtBookStream::Snapshot || self.sequence_number == 0 {
            LobEventKind::Snapshot
        } else if book.bids.is_empty() && book.asks.is_empty() {
            LobEventKind::Heartbeat
        } else {
            LobEventKind::Incremental
        };
        let delete_on_zero = matches!(event, LobEventKind::Incremental);
        let levels = |levels: Vec<GrvtLevel>| {
            levels
                .into_iter()
                .map(|level| {
                    grvt_lob_level(
                        &level.price,
                        &level.size,
                        Some(level.num_orders),
                        delete_on_zero,
                    )
                })
                .collect()
        };

        WsLob {
            timestamp: grvt_ns_to_micros(book.event_time),
            market: GRVT,
            inst: grvt_inst_to_cli(&book.instrument),
            event,
            bids: levels(book.bids),
            asks: levels(book.asks),
            seq: Some(grvt_lob_seq(
                self.sequence_number,
                self.prev_sequence_number,
            )),
            checksum: None,
        }
    }
}

impl IntoWsData for WsMiniTickerGrvt {
    type Output = WsLob;

    fn into_ws(self) -> WsLob {
        let ticker = self.feed;
        let side = |price: &str, size: &str| {
            let level = grvt_lob_level(price, size, None, false);
            if level.price > 0.0 {
                vec![level]
            } else {
                Vec::new()
            }
        };
        let seq = grvt_lob_seq(self.sequence_number, self.prev_sequence_number);

        WsLob {
            timestamp: grvt_ns_to_micros(ticker.event_time),
            market: GRVT,
            inst: grvt_inst_to_cli(&ticker.instrument),
            event: LobEventKind::Bbo,
            bids: side(&ticker.best_bid_price, &ticker.best_bid_size),
            asks: side(&ticker.best_ask_price, &ticker.best_ask_size),
            seq: Some(LobSeq {
                first: seq.last,
                ..seq
            }),
            checksum: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::grvt::grvt_ws_msg::GrvtWsData;

    use super::*;

    const MINI: &str = r#"{"stream":"v1.mini.s","selector":"BTC_USDT_Perp@200","sequence_number":"5308",
        "feed":{"event_time":"1790586065200004186","instrument":"BTC_USDT_Perp","mark_price":"82975.507846723",
        "index_price":"83003.119786761","last_price":"82966.7","last_size":"0.002","mid_price":"82966.35",
        "best_bid_price":"82966.3","best_bid_size":"0.531","best_ask_price":"82966.4","best_ask_size":"3.842"},
        "prev_sequence_number":"5307"}"#;

    fn decode_book(frame: &[u8]) -> WsLob {
        GrvtWsData::<WsBookGrvt>::decode_single(frame)
            .unwrap()
            .into_ws()
            .pop()
            .unwrap()
    }

    fn decode_bbo(frame: &[u8]) -> WsLob {
        GrvtWsData::<WsMiniTickerGrvt>::decode_single(frame)
            .unwrap()
            .into_ws()
            .pop()
            .unwrap()
    }

    #[test]
    fn subscribe_snapshot_is_timed_and_unsequenced() {
        let lob = decode_book(
            br#"{"stream":"v1.book.d","selector":"BTC_USDT_Perp@50","sequence_number":"0","feed":{
            "event_time":"1790586064400000000","instrument":"BTC_USDT_Perp",
            "bids":[{"price":"82966.3","size":"0.531","num_orders":3},{"price":"82964.0","size":"0.663","num_orders":2}],
            "asks":[{"price":"82966.4","size":"3.842","num_orders":7}]},"prev_sequence_number":"0"}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Snapshot));
        assert_eq!(lob.market, GRVT);
        assert_eq!(lob.inst, "BTC_USDT_PERP");
        assert_eq!(lob.timestamp, 1_790_586_064_400_000);
        assert_eq!((lob.bids[0].price, lob.bids[0].size), (82966.3, 0.531));
        assert_eq!(lob.bids[0].order_count, Some(3));
        assert_eq!(lob.asks.len(), 1);
        let seq = lob.seq.unwrap();
        assert_eq!((seq.prev, seq.first, seq.last), (None, None, None));
    }

    #[test]
    fn delta_chains_on_prev_and_deletes_zero_sizes() {
        let lob = decode_book(
            br#"{"stream":"v1.book.d","selector":"BTC_USDT_Perp@50","sequence_number":"36481","feed":{
            "event_time":"1790586064900066293","instrument":"BTC_USDT_Perp",
            "bids":[{"price":"82907.9","size":"1.334","num_orders":1},{"price":"82907.3","size":"0.0","num_orders":0}],
            "asks":[]},"prev_sequence_number":"36480"}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Incremental));
        assert_eq!(lob.timestamp, 1_790_586_064_900_066);
        assert!(matches!(lob.bids[0].action, LobLevelAction::Upsert));
        assert!(matches!(lob.bids[1].action, LobLevelAction::Delete));
        assert_eq!(lob.bids[1].order_count, Some(0));
        assert!(lob.asks.is_empty());
        let seq = lob.seq.unwrap();
        assert_eq!((seq.prev, seq.last), (Some(36480), Some(36481)));
    }

    #[test]
    fn first_delta_of_a_fresh_stream_has_no_prev() {
        let lob = decode_book(
            br#"{"stream":"v1.book.d","selector":"NVDA_USDT_Perp@100","sequence_number":"1","feed":{
            "event_time":"1790585457000042259","instrument":"NVDA_USDT_Perp","bids":[],
            "asks":[{"price":"224.53","size":"191.64","num_orders":1},{"price":"224.66","size":"0.0","num_orders":0}]},
            "prev_sequence_number":"0"}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Incremental));
        assert_eq!(lob.inst, "NVDA_USDT_PERP");
        let seq = lob.seq.unwrap();
        assert_eq!((seq.prev, seq.last), (None, Some(1)));
    }

    #[test]
    fn empty_delta_is_a_heartbeat() {
        let lob = decode_book(
            br#"{"stream":"v1.book.d","selector":"BTC_USDT_Perp@50","sequence_number":"36490","feed":{
            "event_time":"1790586065400000000","instrument":"BTC_USDT_Perp","bids":[],"asks":[]},
            "prev_sequence_number":"36489"}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Heartbeat));
    }

    #[test]
    fn snapshot_stream_is_always_a_snapshot_with_zero_sizes_kept() {
        let lob = decode_book(
            br#"{"stream":"v1.book.s","selector":"BTC_USDT_Perp@500-10","sequence_number":"115171","feed":{
            "event_time":"1790586065000052364","instrument":"BTC_USDT_Perp",
            "bids":[{"price":"82966.3","size":"0.531","num_orders":3},{"price":"82964.0","size":"0.0","num_orders":0}],
            "asks":[{"price":"82966.4","size":"3.842","num_orders":7}]},"prev_sequence_number":"115170"}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Snapshot));
        assert!(matches!(lob.bids[1].action, LobLevelAction::Upsert));
        let seq = lob.seq.unwrap();
        assert_eq!((seq.prev, seq.last), (Some(115170), Some(115171)));
    }

    #[test]
    fn mini_ticker_is_bbo() {
        let lob = decode_bbo(MINI.as_bytes());

        assert!(matches!(lob.event, LobEventKind::Bbo));
        assert_eq!(lob.inst, "BTC_USDT_PERP");
        assert_eq!(lob.timestamp, 1_790_586_065_200_004);
        assert_eq!((lob.bids[0].price, lob.bids[0].size), (82966.3, 0.531));
        assert_eq!((lob.asks[0].price, lob.asks[0].size), (82966.4, 3.842));
        let seq = lob.seq.unwrap();
        assert_eq!(
            (seq.prev, seq.first, seq.last),
            (Some(5307), Some(5308), Some(5308))
        );
    }

    #[test]
    fn empty_side_has_no_level() {
        let lob = decode_bbo(
            br#"{"stream":"v1.mini.s","selector":"IP_USDT_Perp@200","sequence_number":"0","feed":{
            "event_time":"1790585905163690462","instrument":"IP_USDT_Perp","mark_price":"0.310934948",
            "index_price":"0.310828469","last_price":"0.3108","last_size":"104.5","mid_price":"0.0",
            "best_bid_price":"0.0","best_bid_size":"0.0","best_ask_price":"0.0","best_ask_size":"0.0"},
            "prev_sequence_number":"0"}"#,
        );

        assert!(lob.bids.is_empty() && lob.asks.is_empty());
    }

    #[test]
    fn book_and_bbo_decoders_reject_each_other() {
        let book = br#"{"stream":"v1.book.d","selector":"BTC_USDT_Perp@50","sequence_number":"2","feed":{
            "event_time":"1","instrument":"BTC_USDT_Perp","bids":[],"asks":[]},"prev_sequence_number":"1"}"#;
        let delta_ticker = br#"{"stream":"v1.mini.d","selector":"BTC_USDT_Perp@0","sequence_number":"1849216",
            "feed":{"event_time":"1790585480998968846","instrument":"BTC_USDT_Perp","best_ask_size":"3.91"},
            "prev_sequence_number":"1849215"}"#;

        let other_stream = std::str::from_utf8(book)
            .unwrap()
            .replace("v1.book.d", "v1.trade");

        assert!(GrvtWsData::<WsBookGrvt>::decode_single(MINI.as_bytes()).is_err());
        assert!(GrvtWsData::<WsMiniTickerGrvt>::decode_single(book).is_err());
        assert!(GrvtWsData::<WsMiniTickerGrvt>::decode_single(delta_ticker).is_err());
        assert!(GrvtWsData::<WsBookGrvt>::decode_single(other_stream.as_bytes()).is_err());
    }
}
