use serde::Deserialize;

use extrema_infra::{
    arch::market_assets::api_general::ts_to_micros,
    prelude::{IntoWsData, LobEventKind, LobLevel, LobLevelAction, LobSeq, WsLob},
};

use crate::exchange::aster::{api_utils::aster_inst_to_cli, config_assets::ASTER};

#[derive(Clone, Debug, Deserialize)]
#[serde(transparent)]
pub(crate) struct WsBookTickerAster(AsterBookTicker);

#[derive(Clone, Debug, Deserialize)]
#[serde(transparent)]
pub(crate) struct WsPartialDepthAster(AsterDepthBook);

#[derive(Clone, Debug, Deserialize)]
#[serde(transparent)]
pub(crate) struct WsDiffDepthAster(AsterDepthBook);

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
struct AsterBookTicker {
    u: u64,
    s: String,
    b: String,
    B: String,
    a: String,
    A: String,
    T: u64,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
struct AsterDepthBook {
    T: u64,
    s: String,
    U: u64,
    u: u64,
    pu: Option<u64>,
    b: Vec<AsterLobLevel>,
    a: Vec<AsterLobLevel>,
}

#[derive(Clone, Debug, Deserialize)]
struct AsterLobLevel(String, String);

impl AsterBookTicker {
    fn into_ws_lob(self) -> WsLob {
        let update_id = self.u;

        WsLob {
            timestamp: ts_to_micros(self.T),
            market: ASTER,
            inst: aster_inst_to_cli(&self.s),
            event: LobEventKind::Bbo,
            bids: vec![aster_bbo_level(&self.b, &self.B, update_id)],
            asks: vec![aster_bbo_level(&self.a, &self.A, update_id)],
            seq: Some(LobSeq {
                prev: None,
                first: Some(update_id),
                last: Some(update_id),
            }),
            checksum: None,
        }
    }
}

impl AsterDepthBook {
    fn into_ws_lob(self, event: LobEventKind) -> WsLob {
        let is_empty_update = self.b.is_empty() && self.a.is_empty();
        let delete_on_zero = matches!(event, LobEventKind::Incremental);

        WsLob {
            timestamp: ts_to_micros(self.T),
            market: ASTER,
            inst: aster_inst_to_cli(&self.s),
            event: if matches!(event, LobEventKind::Incremental) && is_empty_update {
                LobEventKind::Heartbeat
            } else {
                event
            },
            bids: self
                .b
                .into_iter()
                .map(|level| aster_lob_level(level, delete_on_zero))
                .collect(),
            asks: self
                .a
                .into_iter()
                .map(|level| aster_lob_level(level, delete_on_zero))
                .collect(),
            seq: Some(LobSeq {
                prev: self.pu,
                first: Some(self.U),
                last: Some(self.u),
            }),
            checksum: None,
        }
    }
}

fn aster_bbo_level(price: &str, size: &str, update_id: u64) -> LobLevel {
    let size = size.parse().unwrap_or_default();

    LobLevel {
        price: price.parse().unwrap_or_default(),
        size,
        action: if size == 0.0 {
            LobLevelAction::Delete
        } else {
            LobLevelAction::Upsert
        },
        order_count: None,
        level_update_id: Some(update_id),
    }
}

fn aster_lob_level(level: AsterLobLevel, delete_on_zero: bool) -> LobLevel {
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

impl IntoWsData for WsBookTickerAster {
    type Output = WsLob;

    fn into_ws(self) -> Self::Output {
        self.0.into_ws_lob()
    }
}

impl IntoWsData for WsPartialDepthAster {
    type Output = WsLob;

    fn into_ws(self) -> Self::Output {
        self.0.into_ws_lob(LobEventKind::Snapshot)
    }
}

impl IntoWsData for WsDiffDepthAster {
    type Output = WsLob;

    fn into_ws(self) -> Self::Output {
        self.0.into_ws_lob(LobEventKind::Incremental)
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::aster::aster_ws_msg::AsterWsData;

    use super::*;

    #[test]
    fn book_ticker_is_bbo() {
        let raw =
            br#"{"e":"bookTicker","u":570480683712,"s":"NVDAUSDT","b":"223.460000","B":"1.59",
            "a":"223.520000","A":"0.42","T":1790577909750,"E":1790577909773}"#;

        let lob = AsterWsData::<WsBookTickerAster>::decode_single(raw)
            .unwrap()
            .into_ws();

        assert_eq!(lob.len(), 1);
        assert!(matches!(lob[0].event, LobEventKind::Bbo));
        assert_eq!(lob[0].market, ASTER);
        assert_eq!(lob[0].inst, "NVDA_USDT_PERP");
        assert_eq!(lob[0].timestamp, 1_790_577_909_750_000);
        assert_eq!(lob[0].bids[0].price, 223.46);
        assert_eq!(lob[0].bids[0].size, 1.59);
        assert_eq!(lob[0].asks[0].price, 223.52);
        assert_eq!(lob[0].asks[0].level_update_id, Some(570480683712));
        assert_eq!(lob[0].seq.as_ref().unwrap().last, Some(570480683712));
    }

    #[test]
    fn partial_depth_is_a_snapshot_with_zero_sizes_kept() {
        let raw = br#"{"e":"depthUpdate","E":1790578089305,"T":1790578089250,"s":"NVDAUSDT",
            "U":570484920314,"u":570484924237,"pu":570484918980,
            "b":[["223.520000","1.59"],["223.510000","0.00"]],"a":[["223.570000","0.42"]]}"#;

        let lob = AsterWsData::<WsPartialDepthAster>::decode_single(raw)
            .unwrap()
            .into_ws();

        assert!(matches!(lob[0].event, LobEventKind::Snapshot));
        assert_eq!(lob[0].bids.len(), 2);
        assert!(matches!(lob[0].bids[1].action, LobLevelAction::Upsert));
        let seq = lob[0].seq.as_ref().unwrap();
        assert_eq!(
            (seq.prev, seq.first, seq.last),
            (Some(570484918980), Some(570484920314), Some(570484924237))
        );
    }

    #[test]
    fn diff_depth_zero_size_is_a_delete() {
        let raw = br#"{"e":"depthUpdate","E":1790577907324,"T":1790577907300,"s":"NVDAUSDT",
            "U":570480617005,"u":570480619013,"pu":570480611647,
            "b":[["223.410000","0.00"],["223.420000","228.98"]],"a":[]}"#;

        let lob = AsterWsData::<WsDiffDepthAster>::decode_single(raw)
            .unwrap()
            .into_ws();

        assert!(matches!(lob[0].event, LobEventKind::Incremental));
        assert!(matches!(lob[0].bids[0].action, LobLevelAction::Delete));
        assert!(matches!(lob[0].bids[1].action, LobLevelAction::Upsert));
        assert!(lob[0].asks.is_empty());
    }

    #[test]
    fn empty_diff_is_a_heartbeat() {
        let raw = br#"{"e":"depthUpdate","E":1,"T":1790577907300,"s":"NVDAUSDT",
            "U":5,"u":6,"pu":4,"b":[],"a":[]}"#;

        let lob = AsterWsData::<WsDiffDepthAster>::decode_single(raw)
            .unwrap()
            .into_ws();

        assert!(matches!(lob[0].event, LobEventKind::Heartbeat));
    }

    #[test]
    fn subscription_ack_is_not_a_book() {
        let lob = AsterWsData::<WsDiffDepthAster>::decode_single(br#"{"id":1,"result":null}"#)
            .unwrap()
            .into_ws();

        assert!(lob.is_empty());
    }
}
