use serde::Deserialize;

use extrema_infra::{
    arch::market_assets::api_general::ts_to_micros,
    prelude::{IntoWsData, LobEventKind, LobLevel, LobLevelAction, LobSeq, WsLob},
};

use crate::exchange::apex::{
    apex_ws_msg::ApexFrameKind, api_utils::apex_symbol_to_cli, config_assets::APEX,
};

/// `orderBook{25,200}.H.<SYMBOL>`: a full book on subscribe, then deltas of
/// absolute level sizes.
///
/// `seq.last` is the update id `u`. A delta continues the book only when its
/// `seq.last` is the previous frame's `seq.last` + 1; otherwise resubscribe.
/// A snapshot replaces the book and restarts the chain. ApeX sends snapshot
/// bids worst first; both snapshot sides are re-sorted best price first.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsOrderBookApex {
    #[serde(rename = "type")]
    kind: ApexFrameKind,
    data: ApexBook,
    ts: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct ApexBook {
    s: String,
    b: Vec<ApexLevel>,
    a: Vec<ApexLevel>,
    u: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct ApexLevel(String, String);

fn apex_lob_level(level: ApexLevel, delete_on_zero: bool) -> LobLevel {
    let size = level.1.parse().unwrap_or_default();

    LobLevel {
        price: level.0.parse().unwrap_or_default(),
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

impl IntoWsData for WsOrderBookApex {
    type Output = WsLob;

    fn into_ws(self) -> WsLob {
        let book = self.data;
        let event = match self.kind {
            ApexFrameKind::Snapshot => LobEventKind::Snapshot,
            ApexFrameKind::Delta if book.b.is_empty() && book.a.is_empty() => {
                LobEventKind::Heartbeat
            },
            ApexFrameKind::Delta => LobEventKind::Incremental,
        };
        let delete_on_zero = matches!(event, LobEventKind::Incremental);

        let mut bids: Vec<LobLevel> = book
            .b
            .into_iter()
            .map(|level| apex_lob_level(level, delete_on_zero))
            .collect();
        let mut asks: Vec<LobLevel> = book
            .a
            .into_iter()
            .map(|level| apex_lob_level(level, delete_on_zero))
            .collect();
        if matches!(event, LobEventKind::Snapshot) {
            bids.sort_by(|x, y| y.price.total_cmp(&x.price));
            asks.sort_by(|x, y| x.price.total_cmp(&y.price));
        }

        WsLob {
            timestamp: ts_to_micros(self.ts),
            market: APEX,
            inst: apex_symbol_to_cli(&book.s),
            event,
            bids,
            asks,
            seq: Some(LobSeq {
                prev: None,
                first: Some(book.u),
                last: Some(book.u),
            }),
            checksum: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::apex::apex_ws_msg::ApexWsData;

    use super::*;

    fn decode_book(frame: &[u8]) -> WsLob {
        ApexWsData::<WsOrderBookApex>::decode_single(frame)
            .unwrap()
            .into_ws()
            .pop()
            .unwrap()
    }

    #[test]
    fn snapshot_is_sorted_best_price_first() {
        let lob = decode_book(
            br#"{"topic":"orderBook25.H.NVDAUSDT","type":"snapshot","data":{"s":"NVDAUSDT",
            "b":[["223.98","5.18"],["223.99","214.50"],["224.00","3.46"],["224.03","5.82"],["224.05","247.28"]],
            "a":[["224.17","200.85"],["224.19","11.91"],["224.22","14.39"],["224.23","234.69"],["224.24","1.63"]],
            "u":843412},"cs":66026495617,"ts":1790586413344503}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Snapshot));
        assert_eq!(lob.market, APEX);
        assert_eq!(lob.inst, "NVDA_USDT_PERP");
        assert_eq!(lob.timestamp, 1_790_586_413_344_503);
        let bids: Vec<f64> = lob.bids.iter().map(|l| l.price).collect();
        assert_eq!(bids, vec![224.05, 224.03, 224.0, 223.99, 223.98]);
        assert_eq!((lob.bids[0].size, lob.asks[0].price), (247.28, 224.17));
        assert!(lob.asks.windows(2).all(|w| w[0].price < w[1].price));
        assert!(
            lob.bids
                .iter()
                .all(|l| matches!(l.action, LobLevelAction::Upsert))
        );
        let seq = lob.seq.unwrap();
        assert_eq!(
            (seq.prev, seq.first, seq.last),
            (None, Some(843412), Some(843412))
        );
    }

    #[test]
    fn delta_keeps_frame_order_and_deletes_zero_sizes() {
        let lob = decode_book(
            br#"{"topic":"orderBook25.H.BTCUSDT","type":"delta","data":{"s":"BTCUSDT",
            "b":[["82810.9","0"],["82811.1","0"],["82811.7","0.137"],["82812.1","0.137"]],
            "a":[["83031.9","0"],["82918.6","3.931"]],"u":5234004},
            "cs":66025718952,"ts":1790585479621641}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Incremental));
        assert_eq!(lob.inst, "BTC_USDT_PERP");
        assert_eq!(lob.bids[0].price, 82810.9);
        assert!(matches!(lob.bids[0].action, LobLevelAction::Delete));
        assert!(matches!(lob.bids[2].action, LobLevelAction::Upsert));
        assert_eq!((lob.asks[1].price, lob.asks[1].size), (82918.6, 3.931));
        assert!(matches!(lob.asks[0].action, LobLevelAction::Delete));
        assert_eq!(lob.seq.unwrap().last, Some(5234004));
    }

    #[test]
    fn empty_delta_is_a_heartbeat() {
        let lob = decode_book(
            br#"{"topic":"orderBook25.H.BTCUSDT","type":"delta","data":{"s":"BTCUSDT",
            "b":[],"a":[],"u":5234005},"cs":1,"ts":1790585479673206}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Heartbeat));
        assert_eq!(lob.seq.unwrap().last, Some(5234005));
    }

    #[test]
    fn book_decoder_rejects_other_topics() {
        let trades =
            br#"{"topic":"recentlyTrade.H.BTCUSDT","type":"delta","data":[{"T":1790585689582,
            "s":"BTCUSDT","S":"Buy","v":"0.001","p":"82989.3","L":"PlusTick",
            "i":"0fedbab1-5c23-551f-850f-69a1bbfc9261"}],"cs":66025873033,"ts":1790585689671505}"#;
        let ticker =
            br#"{"topic":"instrumentInfo.H.BTCUSDT","type":"delta","data":{"symbol":"BTCUSDT",
            "indexPrice":"82981","symbolStatus":"0"},"cs":66025831573,"ts":1790585631471558}"#;

        assert!(ApexWsData::<WsOrderBookApex>::decode_single(trades).is_err());
        assert!(ApexWsData::<WsOrderBookApex>::decode_single(ticker).is_err());
    }
}
