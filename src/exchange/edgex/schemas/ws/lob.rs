use serde::Deserialize;

use extrema_infra::{
    arch::market_assets::api_general::{de_u64_from_string_or_number, ts_to_micros},
    prelude::{IntoWsData, LobEventKind, LobLevel, LobLevelAction, LobSeq, WsLob},
};

use crate::exchange::edgex::{
    api_utils::edgex_contract_to_cli, config_assets::EDGEX, edgex_ws_msg::EdgexFrameKind,
};

/// `depth.{id}.{15|200}`: the subscribe reply is the full book, then deltas of
/// absolute level sizes; the server itself deletes levels that leave the window.
///
/// An update continues the book when its `seq.prev` (`startVersion`) equals the
/// previous frame's `seq.last` (`endVersion`); otherwise resubscribe. There is no
/// checksum. Frames carry no exchange time, so `timestamp` is 0; time the book
/// with the BBO or trades streams.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsDepthEdgex {
    #[serde(rename = "type")]
    _kind: EdgexFrameKind,
    content: EdgexDepthContent,
}

/// Every depth frame carries exactly one book.
#[derive(Clone, Debug, Deserialize)]
struct EdgexDepthContent {
    data: [EdgexDepth; 1],
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
struct EdgexDepth {
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    startVersion: u64,
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    endVersion: u64,
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    contractId: u64,
    asks: Vec<EdgexLevel>,
    bids: Vec<EdgexLevel>,
    depthType: EdgexDepthType,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
enum EdgexDepthType {
    Snapshot,
    Changed,
}

#[derive(Clone, Debug, Deserialize)]
struct EdgexLevel {
    price: String,
    size: String,
}

/// One market of `bookTicker.all.1s`; an empty side is quoted as price and size 0.
#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsBookTickerEdgex {
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    contractId: u64,
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    time: u64,
    bestBidPrice: String,
    bestBidSize: String,
    bestAskPrice: String,
    bestAskSize: String,
}

fn edgex_lob_level(price: &str, size: &str, delete_on_zero: bool) -> LobLevel {
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

fn edgex_bbo_side(price: &str, size: &str) -> Vec<LobLevel> {
    let level = edgex_lob_level(price, size, false);

    if level.size > 0.0 {
        vec![level]
    } else {
        Vec::new()
    }
}

impl IntoWsData for WsDepthEdgex {
    type Output = WsLob;

    fn into_ws(self) -> WsLob {
        let [book] = self.content.data;
        let event = match book.depthType {
            EdgexDepthType::Snapshot => LobEventKind::Snapshot,
            EdgexDepthType::Changed if book.bids.is_empty() && book.asks.is_empty() => {
                LobEventKind::Heartbeat
            },
            EdgexDepthType::Changed => LobEventKind::Incremental,
        };
        let delete_on_zero = matches!(event, LobEventKind::Incremental);
        let prev = (!matches!(event, LobEventKind::Snapshot)).then_some(book.startVersion);

        WsLob {
            timestamp: 0,
            market: EDGEX,
            inst: edgex_contract_to_cli(book.contractId),
            event,
            bids: book
                .bids
                .iter()
                .map(|level| edgex_lob_level(&level.price, &level.size, delete_on_zero))
                .collect(),
            asks: book
                .asks
                .iter()
                .map(|level| edgex_lob_level(&level.price, &level.size, delete_on_zero))
                .collect(),
            seq: Some(LobSeq {
                prev,
                first: None,
                last: Some(book.endVersion),
            }),
            checksum: None,
        }
    }
}

impl IntoWsData for WsBookTickerEdgex {
    type Output = WsLob;

    fn into_ws(self) -> WsLob {
        WsLob {
            timestamp: ts_to_micros(self.time),
            market: EDGEX,
            inst: edgex_contract_to_cli(self.contractId),
            event: LobEventKind::Bbo,
            bids: edgex_bbo_side(&self.bestBidPrice, &self.bestBidSize),
            asks: edgex_bbo_side(&self.bestAskPrice, &self.bestAskSize),
            seq: None,
            checksum: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::edgex::edgex_ws_msg::EdgexWsData;

    use super::*;

    fn decode_book(frame: &[u8]) -> WsLob {
        EdgexWsData::<WsDepthEdgex>::decode_single(frame)
            .unwrap()
            .into_ws()
            .pop()
            .unwrap()
    }

    #[test]
    fn subscribe_reply_is_an_untimed_snapshot() {
        let lob = decode_book(
            br#"{"type":"quote-event","channel":"depth.30000001.15","content":{"channel":"depth.30000001.15",
            "dataType":"Snapshot","data":[{"startVersion":"2070150099","endVersion":"2070150189","level":15,
            "contractId":"30000001","contractName":"BTCUSDC",
            "asks":[{"price":"82862.8","size":"0.551"},{"price":"82862.9","size":"0.883"}],
            "bids":[{"price":"82862.5","size":"0.677"}],"depthType":"SNAPSHOT"}]}}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Snapshot));
        assert_eq!(lob.market, EDGEX);
        assert_eq!(lob.inst, "@30000001");
        assert_eq!(lob.timestamp, 0);
        assert_eq!((lob.asks[0].price, lob.asks[0].size), (82862.8, 0.551));
        assert_eq!(lob.bids.len(), 1);
        assert!(matches!(lob.asks[0].action, LobLevelAction::Upsert));
        let seq = lob.seq.unwrap();
        assert_eq!((seq.prev, seq.last), (None, Some(2070150189)));
    }

    #[test]
    fn update_chains_on_versions_and_deletes_zero_sizes() {
        let lob = decode_book(
            br#"{"type":"quote-event","channel":"depth.30000001.15","content":{"channel":"depth.30000001.15",
            "dataType":"changed","data":[{"startVersion":"2070263104","endVersion":"2070263280","level":15,
            "contractId":"30000001","contractName":"BTCUSDC",
            "asks":[{"price":"82899.8","size":"0.000"},{"price":"82899.9","size":"0.912"}],
            "bids":[{"price":"82899.3","size":"0.000"}],"depthType":"CHANGED"}]}}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Incremental));
        assert!(matches!(lob.asks[0].action, LobLevelAction::Delete));
        assert!(matches!(lob.asks[1].action, LobLevelAction::Upsert));
        assert!(matches!(lob.bids[0].action, LobLevelAction::Delete));
        let seq = lob.seq.unwrap();
        assert_eq!((seq.prev, seq.last), (Some(2070263104), Some(2070263280)));
    }

    #[test]
    fn empty_update_is_a_heartbeat() {
        let lob = decode_book(
            br#"{"type":"quote-event","channel":"depth.30000020.15","content":{"channel":"depth.30000020.15",
            "dataType":"changed","data":[{"startVersion":"8","endVersion":"9","level":15,
            "contractId":"30000020","contractName":"NVDAUSDC","asks":[],"bids":[],"depthType":"CHANGED"}]}}"#,
        );

        assert!(matches!(lob.event, LobEventKind::Heartbeat));
        assert_eq!(lob.seq.unwrap().prev, Some(8));
    }

    #[test]
    fn depth_frame_carries_one_known_book() {
        let book = r#"{"startVersion":"1","endVersion":"2","level":15,"contractId":"30000001",
            "contractName":"BTCUSDC","asks":[],"bids":[],"depthType":"CHANGED"}"#;
        let frame = |data: &str| {
            format!(
                r#"{{"type":"quote-event","channel":"depth.30000001.15","content":{{
                "channel":"depth.30000001.15","dataType":"changed","data":[{data}]}}}}"#
            )
        };
        let decode =
            |data: &str| EdgexWsData::<WsDepthEdgex>::decode_single(frame(data).as_bytes());

        assert!(decode(&format!("{book},{book}")).is_err());
        assert!(decode(&book.replace("CHANGED", "DELTA")).is_err());
        assert!(decode("").unwrap().into_ws().is_empty());
    }

    #[test]
    fn book_ticker_batch_is_bbo_per_market() {
        let lobs = EdgexWsData::<WsBookTickerEdgex>::decode_batch(
            br#"{"type":"quote-event","channel":"bookTicker.all.1s","content":{"channel":"bookTicker.all.1s",
            "dataType":"changed","data":[
            {"contractId":"30000001","contractName":"BTCUSDC","time":"1790586196955","bestAskPrice":"82899.7",
             "bestAskSize":"0.763","bestBidPrice":"82899.4","bestBidSize":"1.135"},
            {"contractId":"30000020","contractName":"NVDAUSDC","time":"1790586196049","bestAskPrice":"223.45",
             "bestAskSize":"20.31","bestBidPrice":"223.30","bestBidSize":"16.10"}]}}"#,
        )
        .unwrap()
        .into_ws();

        assert_eq!(lobs.len(), 2);
        assert!(lobs.iter().all(|l| matches!(l.event, LobEventKind::Bbo)));
        assert_eq!(lobs[0].inst, "@30000001");
        assert_eq!(lobs[0].timestamp, 1_790_586_196_955_000);
        assert_eq!((lobs[1].bids[0].price, lobs[1].bids[0].size), (223.3, 16.1));
        assert_eq!(
            (lobs[1].asks[0].price, lobs[1].asks[0].size),
            (223.45, 20.31)
        );
        assert!(lobs[1].seq.is_none());
    }

    #[test]
    fn empty_book_has_empty_bbo_sides() {
        let lob = EdgexWsData::<WsBookTickerEdgex>::decode_batch(
            br#"{"type":"quote-event","channel":"bookTicker.all.1s","content":{"channel":"bookTicker.all.1s",
            "dataType":"Snapshot","data":[{"contractId":"30000158","contractName":"ZROUSDC",
            "time":"1790586196000","bestAskPrice":"0","bestAskSize":"0","bestBidPrice":"0","bestBidSize":"0"}]}}"#,
        )
        .unwrap()
        .into_ws()
        .pop()
        .unwrap();

        assert!(lob.bids.is_empty() && lob.asks.is_empty());
    }

    #[test]
    fn book_and_bbo_decoders_reject_each_other() {
        let bbo = br#"{"type":"quote-event","channel":"bookTicker.all.1s","content":{"channel":"bookTicker.all.1s",
            "dataType":"changed","data":[{"contractId":"30000001","contractName":"BTCUSDC","time":"1",
            "bestAskPrice":"2","bestAskSize":"1","bestBidPrice":"1","bestBidSize":"1"}]}}"#;
        let book = br#"{"type":"quote-event","channel":"depth.30000001.15","content":{"channel":"depth.30000001.15",
            "dataType":"changed","data":[{"startVersion":"1","endVersion":"2","level":15,"contractId":"30000001",
            "contractName":"BTCUSDC","asks":[],"bids":[],"depthType":"CHANGED"}]}}"#;

        assert!(EdgexWsData::<WsDepthEdgex>::decode_single(bbo).is_err());
        assert!(EdgexWsData::<WsBookTickerEdgex>::decode_batch(book).is_err());
    }
}
