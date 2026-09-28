use serde::Deserialize;

use extrema_infra::{
    arch::market_assets::api_general::ts_to_micros,
    prelude::{IntoWsData, LobEventKind, LobLevel, LobLevelAction, LobSeq, WsLob},
};

use crate::exchange::arcus::{
    api_utils::arcus_market_to_cli, arcus_ws_msg::ArcusFrameKind, config_assets::ARCUS,
};

const ARCUS_SNAPSHOT_CHANNEL: &str = "l2Orderbook";

/// `l2Orderbook` frames are all full books. `l2OrderbookUpdates` sends a full
/// book on subscribe, then deltas of absolute level sizes.
///
/// `seq.last` is the per-market sequence. The first delta after a snapshot may
/// skip ahead; after that each delta must be the previous `seq.last` + 1, or
/// the book has to be resubscribed. Deltas carry no exchange time, so their
/// `timestamp` is 0; the `bbo` channel keeps timing the book.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsL2BookArcus {
    #[serde(rename = "type")]
    kind: ArcusFrameKind,
    channel: String,
    id: String,
    contents: ArcusBook,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
struct ArcusBook {
    bids: Vec<ArcusLevel>,
    asks: Vec<ArcusLevel>,
    lastSequenceId: u64,
    #[serde(default)]
    timestamp: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
struct ArcusLevel(String, String);

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsBboArcus {
    id: String,
    contents: ArcusBbo,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
struct ArcusBbo {
    bestBid: Option<ArcusQuote>,
    bestAsk: Option<ArcusQuote>,
    lastSequenceId: u64,
    timestamp: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct ArcusQuote {
    price: String,
    size: String,
}

fn arcus_lob_level(price: &str, size: &str, delete_on_zero: bool) -> LobLevel {
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

impl IntoWsData for WsL2BookArcus {
    type Output = WsLob;

    fn into_ws(self) -> WsLob {
        let book = self.contents;
        let event =
            if self.kind == ArcusFrameKind::Subscribed || self.channel == ARCUS_SNAPSHOT_CHANNEL {
                LobEventKind::Snapshot
            } else if book.bids.is_empty() && book.asks.is_empty() {
                LobEventKind::Heartbeat
            } else {
                LobEventKind::Incremental
            };
        let delete_on_zero = matches!(event, LobEventKind::Incremental);

        WsLob {
            timestamp: book.timestamp.map(ts_to_micros).unwrap_or_default(),
            market: ARCUS,
            inst: arcus_market_to_cli(&self.id),
            event,
            bids: book
                .bids
                .iter()
                .map(|level| arcus_lob_level(&level.0, &level.1, delete_on_zero))
                .collect(),
            asks: book
                .asks
                .iter()
                .map(|level| arcus_lob_level(&level.0, &level.1, delete_on_zero))
                .collect(),
            seq: Some(LobSeq {
                prev: None,
                first: None,
                last: Some(book.lastSequenceId),
            }),
            checksum: None,
        }
    }
}

impl IntoWsData for WsBboArcus {
    type Output = WsLob;

    fn into_ws(self) -> WsLob {
        let quote = |quote: Option<ArcusQuote>| {
            quote
                .map(|q| vec![arcus_lob_level(&q.price, &q.size, false)])
                .unwrap_or_default()
        };

        WsLob {
            timestamp: ts_to_micros(self.contents.timestamp),
            market: ARCUS,
            inst: arcus_market_to_cli(&self.id),
            event: LobEventKind::Bbo,
            bids: quote(self.contents.bestBid),
            asks: quote(self.contents.bestAsk),
            seq: Some(LobSeq {
                prev: None,
                first: Some(self.contents.lastSequenceId),
                last: Some(self.contents.lastSequenceId),
            }),
            checksum: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::arcus::arcus_ws_msg::ArcusWsData;

    use super::*;

    fn decode_book(frame: &[u8]) -> WsLob {
        ArcusWsData::<WsL2BookArcus>::decode_single(frame)
            .unwrap()
            .into_ws()
            .pop()
            .unwrap()
    }

    #[test]
    fn subscribe_reply_is_a_timed_snapshot() {
        let lob = decode_book(
            br#"{"type":"subscribed","channel":"l2OrderbookUpdates","id":"NVDA-USD","contents":{
            "bids":[["223.35","5.2689792"],["223.34","1.4327929"]],"asks":[["223.36","2.2384"]],
            "lastSequenceId":53020893,"globalSequenceId":2615227373,"timestamp":1790577907249181}}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Snapshot));
        assert_eq!(lob.market, ARCUS);
        assert_eq!(lob.inst, "NVDA_USD_PERP");
        assert_eq!(lob.timestamp, 1_790_577_907_249_181);
        assert_eq!(lob.bids.len(), 2);
        assert_eq!(lob.seq.unwrap().last, Some(53020893));
    }

    #[test]
    fn delta_has_no_timestamp_and_deletes_zero_sizes() {
        let lob = decode_book(
            br#"{"type":"channel_data","channel":"l2OrderbookUpdates","id":"NVDA-USD","contents":{
            "bids":[["212.21","0"],["212.18","0.2356489"]],"asks":[],
            "lastSequenceId":53020896,"globalSequenceId":2615229640}}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Incremental));
        assert_eq!(lob.timestamp, 0);
        assert!(matches!(lob.bids[0].action, LobLevelAction::Delete));
        assert!(matches!(lob.bids[1].action, LobLevelAction::Upsert));
        assert_eq!(lob.seq.unwrap().last, Some(53020896));
    }

    #[test]
    fn repeated_price_in_one_delta_keeps_frame_order() {
        let lob = decode_book(
            br#"{"type":"channel_data","channel":"l2OrderbookUpdates","id":"NVDA-USD","contents":{
            "bids":[["223.3","1"],["223.3","0"]],"asks":[],"lastSequenceId":7,"globalSequenceId":8}}"#,
        );

        assert!(matches!(lob.bids[0].action, LobLevelAction::Upsert));
        assert!(matches!(lob.bids[1].action, LobLevelAction::Delete));
    }

    #[test]
    fn periodic_snapshot_channel_is_always_a_snapshot() {
        let lob = decode_book(
            br#"{"type":"channel_data","channel":"l2Orderbook","id":"NVDA-USD","contents":{
            "bids":[["223.38","0.4456327"]],"asks":[["223.42","3.630963"]],
            "lastSequenceId":53021621,"globalSequenceId":2615323105,"timestamp":1790578089549135}}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Snapshot));
        assert_eq!(lob.timestamp, 1_790_578_089_549_135);
    }

    #[test]
    fn bbo_is_top_of_book() {
        let lob = ArcusWsData::<WsBboArcus>::decode_single(
            br#"{"type":"channel_data","channel":"bbo","id":"BTC-USD","contents":{
            "bestBid":{"price":"83136","size":"0.005"},"bestAsk":{"price":"83136.1","size":"0.15613539"},
            "timestamp":1790577907280545,"lastSequenceId":237582451,"globalSequenceId":2615227390}}"#,
        )
        .unwrap()
        .into_ws()
        .pop()
        .unwrap();

        assert!(matches!(lob.event, LobEventKind::Bbo));
        assert_eq!(lob.inst, "BTC_USD_PERP");
        assert_eq!((lob.bids[0].price, lob.asks[0].price), (83136.0, 83136.1));
        assert_eq!(lob.timestamp, 1_790_577_907_280_545);
    }

    #[test]
    fn one_sided_bbo_has_an_empty_side() {
        let lob = ArcusWsData::<WsBboArcus>::decode_single(
            br#"{"type":"channel_data","channel":"bbo","id":"F-USD","contents":{
            "bestBid":null,"bestAsk":{"price":"10","size":"1"},"timestamp":1,"lastSequenceId":2}}"#,
        )
        .unwrap()
        .into_ws()
        .pop()
        .unwrap();

        assert!(lob.bids.is_empty());
        assert_eq!(lob.asks.len(), 1);
    }

    #[test]
    fn book_and_bbo_decoders_reject_each_other() {
        let bbo = br#"{"type":"channel_data","channel":"bbo","id":"BTC-USD","contents":{
            "bestBid":{"price":"1","size":"1"},"bestAsk":{"price":"2","size":"1"},"timestamp":1,"lastSequenceId":2}}"#;
        let book =
            br#"{"type":"channel_data","channel":"l2OrderbookUpdates","id":"BTC-USD","contents":{
            "bids":[],"asks":[],"lastSequenceId":2}}"#;

        assert!(ArcusWsData::<WsL2BookArcus>::decode_single(bbo).is_err());
        assert!(ArcusWsData::<WsBboArcus>::decode_single(book).is_err());
    }
}
