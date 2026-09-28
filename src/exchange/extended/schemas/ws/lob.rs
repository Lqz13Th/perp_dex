use serde::Deserialize;

use extrema_infra::{
    arch::market_assets::api_general::ts_to_micros,
    prelude::{IntoWsData, LobEventKind, LobLevel, LobLevelAction, LobSeq, WsLob},
};

use crate::exchange::extended::{api_utils::extended_market_to_cli, config_assets::EXTENDED};

/// `orderbooks/{market}` opens with the market's latest minutely snapshot, up to a
/// minute old, and replays the 100ms deltas since; a fresh snapshot follows every
/// minute. `orderbooks/{market}?depth=1` pushes the best bid and ask instead, each
/// frame a snapshot, up to every 10ms.
///
/// `seq.last` numbers one connection's frames from 1, snapshots included. A frame
/// whose `seq.last` is not the previous one + 1 breaks the book: reconnect. Prices
/// are not canonical strings (`82653` and `82653.0` are one level); key by value.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsOrderBookExtended {
    #[serde(flatten)]
    update: ExtendedBookUpdate,
    ts: u64,
    seq: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "UPPERCASE")]
enum ExtendedBookUpdate {
    Snapshot(ExtendedBook<ExtendedLevel>),
    Delta(ExtendedBook<ExtendedDeltaLevel>),
}

#[derive(Clone, Debug, Deserialize)]
struct ExtendedBook<L> {
    m: String,
    b: Vec<L>,
    a: Vec<L>,
    d: ExtendedDepth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
enum ExtendedDepth {
    #[serde(rename = "f")]
    Full,
    #[serde(rename = "1")]
    Top,
}

/// `q` is the absolute size.
#[derive(Clone, Debug, Deserialize)]
struct ExtendedLevel {
    p: String,
    q: String,
}

/// `c` is the absolute size; `q` is only the change.
#[derive(Clone, Debug, Deserialize)]
struct ExtendedDeltaLevel {
    p: String,
    c: String,
}

fn extended_lob_level(price: &str, size: &str, delete_on_zero: bool) -> LobLevel {
    let size = size.parse().unwrap_or_default();

    LobLevel {
        price: price.parse().unwrap_or_default(),
        size,
        action: if delete_on_zero && size == 0.0 {
            LobLevelAction::Delete
        } else {
            LobLevelAction::Upsert
        },
        order_count: None,
        level_update_id: None,
    }
}

impl IntoWsData for WsOrderBookExtended {
    type Output = WsLob;

    fn into_ws(self) -> WsLob {
        let (event, market, bids, asks) = match self.update {
            ExtendedBookUpdate::Snapshot(book) => (
                match book.d {
                    ExtendedDepth::Top => LobEventKind::Bbo,
                    ExtendedDepth::Full => LobEventKind::Snapshot,
                },
                book.m,
                book.b
                    .iter()
                    .map(|level| extended_lob_level(&level.p, &level.q, false))
                    .collect(),
                book.a
                    .iter()
                    .map(|level| extended_lob_level(&level.p, &level.q, false))
                    .collect(),
            ),
            ExtendedBookUpdate::Delta(book) => (
                if book.b.is_empty() && book.a.is_empty() {
                    LobEventKind::Heartbeat
                } else {
                    LobEventKind::Incremental
                },
                book.m,
                book.b
                    .iter()
                    .map(|level| extended_lob_level(&level.p, &level.c, true))
                    .collect(),
                book.a
                    .iter()
                    .map(|level| extended_lob_level(&level.p, &level.c, true))
                    .collect(),
            ),
        };
        let first = matches!(event, LobEventKind::Bbo).then_some(self.seq);

        WsLob {
            timestamp: ts_to_micros(self.ts),
            market: EXTENDED,
            inst: extended_market_to_cli(&market),
            event,
            bids,
            asks,
            seq: Some(LobSeq {
                prev: None,
                first,
                last: Some(self.seq),
            }),
            checksum: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::extended::extended_ws_msg::ExtendedWsData;

    use super::*;

    fn decode(frame: &str) -> WsLob {
        ExtendedWsData::<WsOrderBookExtended>::decode_single(frame.as_bytes())
            .unwrap()
            .into_ws()
            .pop()
            .unwrap()
    }

    #[test]
    fn first_frame_is_a_timed_snapshot() {
        let lob = decode(
            r#"{"type":"SNAPSHOT","data":{"t":"SNAPSHOT","m":"NVDA_24_5-USD","b":[{"q":"1.79","p":"224.29"},
            {"q":"0.64","p":"224.28"},{"q":"44.14","p":"224.27"}],"a":[{"q":"0.67","p":"224.36"},
            {"q":"8.95","p":"224.38"}],"d":"f"},"ts":1790586502600,"seq":1}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Snapshot));
        assert_eq!(lob.market, EXTENDED);
        assert_eq!(lob.inst, "NVDA_24_5_USD_PERP");
        assert_eq!(lob.timestamp, 1_790_586_502_600_000);
        assert_eq!((lob.bids[0].price, lob.bids[0].size), (224.29, 1.79));
        assert_eq!((lob.asks[1].price, lob.asks[1].size), (224.38, 8.95));
        assert!(
            lob.bids
                .iter()
                .all(|l| matches!(l.action, LobLevelAction::Upsert))
        );
        let seq = lob.seq.unwrap();
        assert_eq!((seq.prev, seq.first, seq.last), (None, None, Some(1)));
    }

    #[test]
    fn delta_sizes_are_absolute_and_zero_deletes() {
        let lob = decode(
            r#"{"type":"DELTA","data":{"t":"DELTA","m":"NVDA_24_5-USD","b":[{"q":"-0.64","p":"224.28","c":"0"},
            {"q":"-1.28","p":"224.23","c":"0"}],"a":[{"q":"-0.67","p":"224.36","c":"0"},
            {"q":"-4.44","p":"224.47","c":"17.90"}],"d":"f"},"ts":1790586509857,"seq":18}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Incremental));
        assert!(
            lob.bids
                .iter()
                .all(|l| matches!(l.action, LobLevelAction::Delete))
        );
        assert!(matches!(lob.asks[0].action, LobLevelAction::Delete));
        assert!(matches!(lob.asks[1].action, LobLevelAction::Upsert));
        assert_eq!((lob.asks[1].price, lob.asks[1].size), (224.47, 17.9));
        assert_eq!(lob.seq.unwrap().last, Some(18));
    }

    #[test]
    fn empty_delta_is_a_heartbeat() {
        let lob = decode(
            r#"{"type":"DELTA","data":{"t":"DELTA","m":"NVDA_24_5-USD","b":[],"a":[],"d":"f"},
            "ts":1790586563198,"seq":174}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Heartbeat));
        assert_eq!(lob.seq.unwrap().last, Some(174));
    }

    #[test]
    fn minutely_snapshot_continues_the_sequence() {
        let lob = decode(
            r#"{"type":"SNAPSHOT","data":{"t":"SNAPSHOT","m":"NVDA_24_5-USD","b":[{"q":"0.66","p":"224.1"},
            {"q":"1.33","p":"224.09"}],"a":[{"q":"9.58","p":"224.26"}],"d":"f"},"ts":1790586563198,"seq":175}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Snapshot));
        assert_eq!(lob.seq.unwrap().last, Some(175));
    }

    #[test]
    fn depth_one_frames_are_bbo() {
        let lob = decode(
            r#"{"type":"SNAPSHOT","data":{"t":"SNAPSHOT","m":"BTC-USD","b":[{"q":"4.09776","p":"82588"}],
            "a":[{"q":"0.00011","p":"82590"}],"d":"1"},"ts":1790586554509,"seq":2}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Bbo));
        assert_eq!(lob.inst, "BTC_USD_PERP");
        assert_eq!((lob.bids[0].price, lob.asks[0].price), (82588.0, 82590.0));
        assert_eq!(lob.timestamp, 1_790_586_554_509_000);
        let seq = lob.seq.unwrap();
        assert_eq!((seq.first, seq.last), (Some(2), Some(2)));
    }

    #[test]
    fn delta_without_absolute_sizes_fails_the_frame() {
        let frame = r#"{"type":"DELTA","data":{"t":"DELTA","m":"BTC-USD","b":[{"q":"-0.0001","p":"82648"}],
            "a":[],"d":"f"},"ts":1790586503423,"seq":2}"#;

        assert!(ExtendedWsData::<WsOrderBookExtended>::decode_single(frame.as_bytes()).is_err());
    }

    #[test]
    fn trade_and_mark_frames_are_not_books() {
        for frame in [
            r#"{"data":[{"i":2104498657029001217,"m":"BTC-USD","S":"BUY","tT":"TRADE","T":1790586554187,
            "p":"82589","q":"0.30270"}],"ts":1790586554191,"seq":2}"#,
            r#"{"type":"MP","data":{"m":"BTC-USD","p":"82603.900514499997","ts":0},"ts":1790586555172,"seq":1}"#,
            r#"{"type":"SNAPSHOT","data":{"t":"SNAPSHOT","m":"BTC-USD","b":[],"a":[],"d":"5"},"ts":1,"seq":1}"#,
        ] {
            assert!(
                ExtendedWsData::<WsOrderBookExtended>::decode_single(frame.as_bytes()).is_err(),
                "{frame}"
            );
        }
    }
}
