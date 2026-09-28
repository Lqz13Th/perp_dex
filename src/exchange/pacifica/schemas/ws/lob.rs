use serde::Deserialize;

use extrema_infra::{
    arch::market_assets::api_general::ts_to_micros,
    prelude::{IntoWsData, LobEventKind, LobLevel, LobLevelAction, LobSeq, WsLob},
};

use crate::exchange::pacifica::{api_utils::pacifica_symbol_to_cli, config_assets::PACIFICA};

/// `book` frames are all full aggregated books, ten levels a side, about every 250ms.
///
/// `seq.last` is the per-symbol last id `li`: it never decreases, and repeats
/// while the book is unchanged, so an older `li` means a stale frame. `timestamp`
/// is the book's last change, so it can lag the frame by seconds on a quiet market.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsBookPacifica {
    s: String,
    l: (Vec<PacificaLevel>, Vec<PacificaLevel>),
    t: u64,
    li: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct PacificaLevel {
    p: String,
    a: String,
    n: u64,
}

/// `bbo` frames come on every top-of-book change; `li` rises with each.
#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsBboPacifica {
    s: String,
    li: u64,
    t: u64,
    b: String,
    B: String,
    a: String,
    A: String,
}

fn pacifica_lob_level(price: &str, size: &str, order_count: Option<u64>) -> LobLevel {
    LobLevel {
        price: price.parse().unwrap_or_default(),
        size: size.parse().unwrap_or_default(),
        action: LobLevelAction::Upsert,
        order_count,
        level_update_id: None,
    }
}

impl IntoWsData for WsBookPacifica {
    type Output = WsLob;

    fn into_ws(self) -> WsLob {
        let (bids, asks) = self.l;
        let levels = |levels: Vec<PacificaLevel>| {
            levels
                .iter()
                .map(|level| pacifica_lob_level(&level.p, &level.a, Some(level.n)))
                .collect()
        };

        WsLob {
            timestamp: ts_to_micros(self.t),
            market: PACIFICA,
            inst: pacifica_symbol_to_cli(&self.s),
            event: LobEventKind::Snapshot,
            bids: levels(bids),
            asks: levels(asks),
            seq: Some(LobSeq {
                prev: None,
                first: None,
                last: Some(self.li),
            }),
            checksum: None,
        }
    }
}

impl IntoWsData for WsBboPacifica {
    type Output = WsLob;

    fn into_ws(self) -> WsLob {
        WsLob {
            timestamp: ts_to_micros(self.t),
            market: PACIFICA,
            inst: pacifica_symbol_to_cli(&self.s),
            event: LobEventKind::Bbo,
            bids: vec![pacifica_lob_level(&self.b, &self.B, None)],
            asks: vec![pacifica_lob_level(&self.a, &self.A, None)],
            seq: Some(LobSeq {
                prev: None,
                first: Some(self.li),
                last: Some(self.li),
            }),
            checksum: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::pacifica::pacifica_ws_msg::PacificaWsData;

    use super::*;

    const BOOK: &[u8] = br#"{"channel":"book","data":{"s":"BTC","l":[[{"p":"82939","a":"0.19187","n":4},
        {"p":"82938","a":"0.01205","n":1},{"p":"82937","a":"0.01205","n":1}],[{"p":"82940","a":"0.77094","n":3},
        {"p":"82941","a":"0.01206","n":1}]],"t":1790585999477,"li":13379811172}}"#;

    const BBO: &[u8] =
        br#"{"channel":"bbo","data":{"s":"SOL-USDC","i":13092816419,"li":13379820558,
        "t":1790586004367,"b":"118.22","B":"0.102","a":"118.48","A":"26.521"}}"#;

    fn decode_book(frame: &[u8]) -> WsLob {
        PacificaWsData::<WsBookPacifica>::decode_single(frame)
            .unwrap()
            .into_ws()
            .pop()
            .unwrap()
    }

    #[test]
    fn every_book_frame_is_a_snapshot_sequenced_by_last_id() {
        let lob = decode_book(BOOK);

        assert!(matches!(lob.event, LobEventKind::Snapshot));
        assert_eq!(lob.market, PACIFICA);
        assert_eq!(lob.inst, "BTC_USDC_PERP");
        assert_eq!(lob.timestamp, 1_790_585_999_477_000);
        assert_eq!((lob.bids.len(), lob.asks.len()), (3, 2));
        assert_eq!((lob.bids[0].price, lob.bids[0].size), (82939.0, 0.19187));
        assert_eq!(lob.bids[0].order_count, Some(4));
        assert_eq!(lob.asks[0].price, 82940.0);
        assert!(
            lob.bids
                .iter()
                .chain(&lob.asks)
                .all(|l| matches!(l.action, LobLevelAction::Upsert))
        );
        let seq = lob.seq.unwrap();
        assert_eq!(
            (seq.prev, seq.first, seq.last),
            (None, None, Some(13379811172))
        );
    }

    #[test]
    fn empty_book_is_still_a_snapshot() {
        let lob =
            decode_book(br#"{"channel":"book","data":{"s":"PONS","l":[[],[]],"t":1,"li":2}}"#);

        assert!(matches!(lob.event, LobEventKind::Snapshot));
        assert!(lob.bids.is_empty() && lob.asks.is_empty());
    }

    #[test]
    fn bbo_is_top_of_book() {
        let lob = PacificaWsData::<WsBboPacifica>::decode_single(BBO)
            .unwrap()
            .into_ws()
            .pop()
            .unwrap();

        assert!(matches!(lob.event, LobEventKind::Bbo));
        assert_eq!(lob.inst, "SOL_USDC");
        assert_eq!((lob.bids[0].price, lob.bids[0].size), (118.22, 0.102));
        assert_eq!((lob.asks[0].price, lob.asks[0].size), (118.48, 26.521));
        assert_eq!(lob.timestamp, 1_790_586_004_367_000);
        let seq = lob.seq.unwrap();
        assert_eq!(
            (seq.first, seq.last),
            (Some(13379820558), Some(13379820558))
        );
    }

    #[test]
    fn book_and_bbo_decoders_reject_each_other() {
        assert!(PacificaWsData::<WsBookPacifica>::decode_single(BBO).is_err());
        assert!(PacificaWsData::<WsBboPacifica>::decode_single(BOOK).is_err());
    }

    #[test]
    fn missing_last_id_fails_the_frame() {
        let frame = String::from_utf8_lossy(BOOK).replace(r#","li":13379811172"#, "");

        assert!(PacificaWsData::<WsBookPacifica>::decode_single(frame.as_bytes()).is_err());
    }
}
