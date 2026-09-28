use serde::Deserialize;

use extrema_infra::{
    arch::market_assets::api_general::de_u64_from_string_or_number,
    prelude::{IntoWsData, LobEventKind, LobLevel, LobLevelAction, LobSeq, WsLob},
};

use crate::exchange::nado::{
    api_utils::{nado_ns_to_micros, nado_product_to_cli, nado_x18_to_f64},
    config_assets::NADO,
};

/// `book_depth` sends no snapshot: every frame is the absolute size of the levels
/// that changed in a ~50ms batch, zero meaning the level is gone.
///
/// `seq` holds the batch's nanosecond times: `first` = `min_timestamp`, `last` =
/// `max_timestamp`, `prev` = `last_max_timestamp`. A frame continues the book when
/// its `seq.prev` equals the previous frame's `seq.last`; otherwise resync. Seed
/// the book from `NadoCli::get_market_liquidity` and apply the frames whose
/// `seq.last` is above its nanosecond `timestamp`.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsBookDepthNado {
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    min_timestamp: u64,
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    max_timestamp: u64,
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    last_max_timestamp: u64,
    product_id: u32,
    bids: Vec<NadoLevel>,
    asks: Vec<NadoLevel>,
}

#[derive(Clone, Debug, Deserialize)]
struct NadoLevel(String, String);

/// A side without orders has a zero quantity, and its price is `0` or `i128::MAX`.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsBboNado {
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    timestamp: u64,
    product_id: u32,
    bid_price: String,
    bid_qty: String,
    ask_price: String,
    ask_qty: String,
}

fn nado_lob_level(price: &str, size: &str, delete_on_zero: bool) -> LobLevel {
    let size = nado_x18_to_f64(size);

    LobLevel {
        price: nado_x18_to_f64(price),
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

fn nado_bbo_side(price: &str, size: &str) -> Vec<LobLevel> {
    let level = nado_lob_level(price, size, false);

    if level.size > 0.0 {
        vec![level]
    } else {
        Vec::new()
    }
}

impl IntoWsData for WsBookDepthNado {
    type Output = WsLob;

    fn into_ws(self) -> WsLob {
        let event = if self.bids.is_empty() && self.asks.is_empty() {
            LobEventKind::Heartbeat
        } else {
            LobEventKind::Incremental
        };

        WsLob {
            timestamp: nado_ns_to_micros(self.max_timestamp),
            market: NADO,
            inst: nado_product_to_cli(self.product_id),
            event,
            bids: self
                .bids
                .iter()
                .map(|level| nado_lob_level(&level.0, &level.1, true))
                .collect(),
            asks: self
                .asks
                .iter()
                .map(|level| nado_lob_level(&level.0, &level.1, true))
                .collect(),
            seq: Some(LobSeq {
                prev: (self.last_max_timestamp > 0).then_some(self.last_max_timestamp),
                first: Some(self.min_timestamp),
                last: Some(self.max_timestamp),
            }),
            checksum: None,
        }
    }
}

impl IntoWsData for WsBboNado {
    type Output = WsLob;

    fn into_ws(self) -> WsLob {
        WsLob {
            timestamp: nado_ns_to_micros(self.timestamp),
            market: NADO,
            inst: nado_product_to_cli(self.product_id),
            event: LobEventKind::Bbo,
            bids: nado_bbo_side(&self.bid_price, &self.bid_qty),
            asks: nado_bbo_side(&self.ask_price, &self.ask_qty),
            seq: None,
            checksum: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::nado::nado_ws_msg::NadoWsData;

    use super::*;

    fn decode_book(frame: &[u8]) -> WsLob {
        NadoWsData::<WsBookDepthNado>::decode_single(frame)
            .unwrap()
            .into_ws()
            .pop()
            .unwrap()
    }

    fn decode_bbo(frame: &[u8]) -> WsLob {
        NadoWsData::<WsBboNado>::decode_single(frame)
            .unwrap()
            .into_ws()
            .pop()
            .unwrap()
    }

    #[test]
    fn book_depth_is_an_incremental_chained_by_timestamps() {
        let lob = decode_book(
            br#"{"type":"book_depth","last_max_timestamp":"1790585975679078357","min_timestamp":"1790585975707633942",
            "max_timestamp":"1790585975732686867","product_id":2,"bids":[["82913000000000000000000","60300000000000000"]],
            "asks":[["82929000000000000000000","1964500000000000000"],["82931000000000000000000","24500000000000000"],
            ["82946000000000000000000","3048350000000000000"],["82956000000000000000000","0"]]}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Incremental));
        assert_eq!(lob.market, NADO);
        assert_eq!(lob.inst, "@2");
        assert_eq!(lob.timestamp, 1_790_585_975_732_686);
        assert_eq!((lob.bids[0].price, lob.bids[0].size), (82913.0, 0.0603));
        assert_eq!((lob.asks[0].price, lob.asks[0].size), (82929.0, 1.9645));
        assert!(matches!(lob.bids[0].action, LobLevelAction::Upsert));
        assert!(matches!(lob.asks[3].action, LobLevelAction::Delete));
        assert_eq!(lob.asks[3].size, 0.0);
        let seq = lob.seq.unwrap();
        assert_eq!(
            (seq.prev, seq.first, seq.last),
            (
                Some(1_790_585_975_679_078_357),
                Some(1_790_585_975_707_633_942),
                Some(1_790_585_975_732_686_867)
            )
        );
    }

    #[test]
    fn spot_book_depth_keeps_its_product() {
        let lob = decode_book(
            br#"{"type":"book_depth","last_max_timestamp":"1790585981594995122","min_timestamp":"1790585981737148056",
            "max_timestamp":"1790585981737148056","product_id":1,"bids":[["82866000000000000000000","48150000000000000"]],"asks":[]}"#,
        );

        assert_eq!(lob.inst, "@1");
        assert_eq!(lob.bids[0].size, 0.04815);
        assert!(lob.asks.is_empty());
    }

    #[test]
    fn empty_book_depth_is_a_heartbeat() {
        let lob = decode_book(
            br#"{"type":"book_depth","last_max_timestamp":"8","min_timestamp":"9","max_timestamp":"9",
            "product_id":2,"bids":[],"asks":[]}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Heartbeat));
    }

    #[test]
    fn first_book_depth_without_a_predecessor_has_no_prev() {
        let lob = decode_book(
            br#"{"type":"book_depth","last_max_timestamp":"0","min_timestamp":"9","max_timestamp":"9",
            "product_id":2,"bids":[["1000000000000000000","1"]],"asks":[]}"#,
        );

        assert_eq!(lob.seq.unwrap().prev, None);
    }

    #[test]
    fn best_bid_offer_is_bbo() {
        let lob = decode_bbo(
            br#"{"type":"best_bid_offer","timestamp":"1790586210465990606","product_id":112,"bid_price":"223920000000000000000",
            "bid_qty":"2900000000000000000","ask_price":"224000000000000000000","ask_qty":"3050000000000000000"}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Bbo));
        assert_eq!(lob.inst, "@112");
        assert_eq!(lob.timestamp, 1_790_586_210_465_990);
        assert_eq!((lob.bids[0].price, lob.bids[0].size), (223.92, 2.9));
        assert_eq!((lob.asks[0].price, lob.asks[0].size), (224.0, 3.05));
        assert!(lob.seq.is_none());
    }

    #[test]
    fn empty_bbo_sides_are_dropped() {
        let lob = decode_bbo(
            br#"{"type":"best_bid_offer","timestamp":"1","product_id":184,"bid_price":"0","bid_qty":"0",
            "ask_price":"170141183460469231731687303715884105727","ask_qty":"0"}"#,
        );

        assert!(lob.bids.is_empty() && lob.asks.is_empty());
    }

    #[test]
    fn book_and_bbo_decoders_reject_each_other() {
        let bbo = br#"{"type":"best_bid_offer","timestamp":"1","product_id":2,"bid_price":"1","bid_qty":"1",
            "ask_price":"2","ask_qty":"1"}"#;
        let book = br#"{"type":"book_depth","last_max_timestamp":"1","min_timestamp":"2","max_timestamp":"2",
            "product_id":2,"bids":[],"asks":[]}"#;

        assert!(NadoWsData::<WsBookDepthNado>::decode_single(bbo).is_err());
        assert!(NadoWsData::<WsBboNado>::decode_single(book).is_err());
    }

    #[test]
    fn non_numeric_timestamps_fail_the_frame() {
        assert!(
            NadoWsData::<WsBookDepthNado>::decode_single(
                br#"{"type":"book_depth","last_max_timestamp":"x","min_timestamp":"2","max_timestamp":"2",
                "product_id":2,"bids":[],"asks":[]}"#
            )
            .is_err()
        );
    }
}
