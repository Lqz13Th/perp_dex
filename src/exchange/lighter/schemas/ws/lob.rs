use serde::Deserialize;

use extrema_infra::{
    arch::market_assets::api_general::ts_to_micros,
    prelude::{IntoWsData, LobEventKind, LobLevel, LobLevelAction, LobSeq, Market, WsLob},
};

use crate::exchange::lighter::api_utils::lighter_channel_to_cli;

/// `order_book/{id}`: the subscribe reply is the full book, updates are 50ms batches.
///
/// An update continues the book when its `seq.prev` (`begin_nonce`) equals the
/// previous frame's `seq.last` (`nonce`); otherwise resubscribe.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsOrderBookLighter<const ID: u16> {
    channel: String,
    #[serde(rename = "type")]
    kind: String,
    order_book: LighterBook,
}

#[derive(Clone, Debug, Deserialize)]
struct LighterBook {
    asks: Vec<LighterLevel>,
    bids: Vec<LighterLevel>,
    nonce: u64,
    begin_nonce: u64,
    last_updated_at: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsTickerLighter<const ID: u16> {
    channel: String,
    nonce: u64,
    ticker: LighterTicker,
}

#[derive(Clone, Debug, Deserialize)]
struct LighterTicker {
    a: LighterLevel,
    b: LighterLevel,
    last_updated_at: u64,
}

#[derive(Clone, Debug, Deserialize)]
struct LighterLevel {
    price: String,
    size: String,
}

fn lighter_lob_level(level: LighterLevel, delete_on_zero: bool) -> LobLevel {
    let size = level.size.parse().unwrap_or_default();

    LobLevel {
        price: level.price.parse().unwrap_or_default(),
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

impl<const ID: u16> IntoWsData for WsOrderBookLighter<ID> {
    type Output = WsLob;

    fn into_ws(self) -> WsLob {
        let book = self.order_book;
        let event = if self.kind.starts_with("subscribed/") {
            LobEventKind::Snapshot
        } else if book.asks.is_empty() && book.bids.is_empty() {
            LobEventKind::Heartbeat
        } else {
            LobEventKind::Incremental
        };
        let delete_on_zero = matches!(event, LobEventKind::Incremental);

        WsLob {
            timestamp: ts_to_micros(book.last_updated_at),
            market: Market::Custom(ID),
            inst: lighter_channel_to_cli(&self.channel),
            event,
            bids: book
                .bids
                .into_iter()
                .map(|level| lighter_lob_level(level, delete_on_zero))
                .collect(),
            asks: book
                .asks
                .into_iter()
                .map(|level| lighter_lob_level(level, delete_on_zero))
                .collect(),
            seq: Some(LobSeq {
                prev: (book.begin_nonce > 0).then_some(book.begin_nonce),
                first: None,
                last: Some(book.nonce),
            }),
            checksum: None,
        }
    }
}

impl<const ID: u16> IntoWsData for WsTickerLighter<ID> {
    type Output = WsLob;

    fn into_ws(self) -> WsLob {
        WsLob {
            timestamp: ts_to_micros(self.ticker.last_updated_at),
            market: Market::Custom(ID),
            inst: lighter_channel_to_cli(&self.channel),
            event: LobEventKind::Bbo,
            bids: vec![lighter_lob_level(self.ticker.b, false)],
            asks: vec![lighter_lob_level(self.ticker.a, false)],
            seq: Some(LobSeq {
                prev: None,
                first: Some(self.nonce),
                last: Some(self.nonce),
            }),
            checksum: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::lighter::{
        config_assets::{LIGHTER, LIGHTER_MARKET_ID, LIGHTER_RH, LIGHTER_RH_MARKET_ID},
        lighter_ws_msg::LighterWsData,
    };

    use super::*;

    fn decode_book(frame: &[u8]) -> WsLob {
        LighterWsData::<WsOrderBookLighter<LIGHTER_MARKET_ID>>::decode_single(frame)
            .unwrap()
            .into_ws()
            .pop()
            .unwrap()
    }

    #[test]
    fn subscribe_reply_is_a_snapshot() {
        let lob = decode_book(
            br#"{"channel":"order_book:110","last_updated_at":1790577907186668,"offset":451523,
            "order_book":{"code":0,"asks":[{"price":"223.363","size":"28.627"}],
            "bids":[{"price":"223.339","size":"2.705"},{"price":"223.300","size":"1.000"}],
            "offset":451523,"nonce":23189369265,"last_updated_at":1790577907186668,"begin_nonce":0},
            "timestamp":1790577907239,"type":"subscribed/order_book"}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Snapshot));
        assert_eq!(lob.market, LIGHTER);
        assert_eq!(lob.inst, "@110");
        assert_eq!(lob.timestamp, 1_790_577_907_186_668);
        assert_eq!(lob.asks[0].price, 223.363);
        assert_eq!(lob.bids.len(), 2);
        let seq = lob.seq.unwrap();
        assert_eq!((seq.prev, seq.last), (None, Some(23189369265)));
    }

    #[test]
    fn update_chains_on_begin_nonce_and_deletes_zero_sizes() {
        let lob = decode_book(
            br#"{"channel":"order_book:1","last_updated_at":1790577907351721,"offset":8106615,
            "order_book":{"code":0,"asks":[{"price":"83136.4","size":"0.73085"},{"price":"83140.4","size":"0.00000"}],
            "bids":[{"price":"83134.7","size":"0.00000"}],"offset":8106615,"nonce":23189369443,
            "last_updated_at":1790577907351721,"begin_nonce":23189369125},
            "timestamp":1790577907378,"type":"update/order_book"}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Incremental));
        assert_eq!(lob.inst, "@1");
        assert!(matches!(lob.asks[0].action, LobLevelAction::Upsert));
        assert!(matches!(lob.asks[1].action, LobLevelAction::Delete));
        assert!(matches!(lob.bids[0].action, LobLevelAction::Delete));
        let seq = lob.seq.unwrap();
        assert_eq!((seq.prev, seq.last), (Some(23189369125), Some(23189369443)));
    }

    #[test]
    fn empty_update_is_a_heartbeat() {
        let lob = decode_book(
            br#"{"channel":"order_book:1","order_book":{"asks":[],"bids":[],"nonce":9,
            "last_updated_at":1790577907351721,"begin_nonce":8},"type":"update/order_book"}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Heartbeat));
    }

    #[test]
    fn ticker_is_bbo() {
        let lob = LighterWsData::<WsTickerLighter<LIGHTER_MARKET_ID>>::decode_single(
            br#"{"channel":"ticker:110","last_updated_at":1790577908278771,"nonce":23189370611,
            "ticker":{"s":"NVDA","a":{"price":"223.363","size":"28.627"},"b":{"price":"223.335","size":"15.888"},
            "last_updated_at":1790577908278771},"timestamp":1790577908280,"type":"update/ticker"}"#,
        )
        .unwrap()
        .into_ws()
        .pop()
        .unwrap();

        assert!(matches!(lob.event, LobEventKind::Bbo));
        assert_eq!(lob.inst, "@110");
        assert_eq!((lob.bids[0].price, lob.bids[0].size), (223.335, 15.888));
        assert_eq!((lob.asks[0].price, lob.asks[0].size), (223.363, 28.627));
        assert_eq!(lob.seq.unwrap().last, Some(23189370611));
    }

    #[test]
    fn robinhood_frames_carry_the_robinhood_market() {
        let lob = LighterWsData::<WsTickerLighter<LIGHTER_RH_MARKET_ID>>::decode_single(
            br#"{"channel":"ticker:16","last_updated_at":1790584459273199,"nonce":2528829567,
            "ticker":{"s":"TSLA","a":{"price":"368.94","size":"16.28"},"b":{"price":"368.90","size":"1.2"},
            "last_updated_at":1790584459273199},"timestamp":1790584459281,"type":"update/ticker"}"#,
        )
        .unwrap()
        .into_ws()
        .pop()
        .unwrap();

        assert_eq!((lob.market, lob.inst.as_str()), (LIGHTER_RH, "@16"));
    }

    #[test]
    fn book_decoder_rejects_ticker_frames() {
        assert!(
            LighterWsData::<WsOrderBookLighter<LIGHTER_MARKET_ID>>::decode_single(
                br#"{"channel":"ticker:110","nonce":1,"ticker":{"a":{"price":"1","size":"1"},
                "b":{"price":"1","size":"1"},"last_updated_at":1},"type":"update/ticker"}"#
            )
            .is_err()
        );
    }
}
