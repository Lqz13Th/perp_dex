use serde::{Deserialize, de::DeserializeOwned};
use tracing::{info, warn};

use extrema_infra::prelude::IntoWsData;

use crate::exchange::ws_decode::decode_preferred;

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum EdgexWsData<T> {
    ChannelSingle(T),
    ChannelBatch(EdgexWsQuote<T>),
    Trades(EdgexWsQuote<T>),
    Event(EdgexWsEvent),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum EdgexFrameKind {
    #[serde(rename = "quote-event")]
    QuoteEvent,
}

/// `Snapshot` is sent once on subscribe; later frames are `changed` (the docs say `Changed`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum EdgexDataType {
    Snapshot,
    #[serde(rename = "changed", alias = "Changed")]
    Changed,
}

#[derive(Clone, Debug, Deserialize)]
pub struct EdgexWsQuote<T> {
    #[serde(rename = "type")]
    _kind: EdgexFrameKind,
    pub content: EdgexWsContent<T>,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct EdgexWsContent<T> {
    pub dataType: EdgexDataType,
    pub data: Vec<T>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum EdgexWsEvent {
    Connected {
        sid: String,
    },
    Subscribed {
        channel: String,
    },
    Unsubscribed {
        channel: String,
    },
    /// Sent every 10 s; the server keeps the connection without the `pong` it asks for.
    Ping,
    Error {
        #[serde(default)]
        request: Option<String>,
        content: EdgexWsError,
    },
}

#[derive(Clone, Debug, Deserialize)]
pub struct EdgexWsError {
    pub code: String,
    pub msg: String,
}

impl<T: DeserializeOwned> EdgexWsData<T> {
    pub(crate) fn decode_single(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::ChannelSingle)
    }

    pub(crate) fn decode_batch(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::ChannelBatch)
    }

    pub(crate) fn decode_trades(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::Trades)
    }
}

impl<T> IntoWsData for EdgexWsData<T>
where
    T: IntoWsData + for<'de> Deserialize<'de>,
{
    type Output = Vec<T::Output>;

    fn into_ws(self) -> Self::Output {
        match self {
            EdgexWsData::ChannelSingle(c) => vec![c.into_ws()],
            EdgexWsData::ChannelBatch(q) => q.content.data.into_iter().map(T::into_ws).collect(),
            // The subscribe reply replays recent fills; only live prints become events.
            EdgexWsData::Trades(q) if q.content.dataType == EdgexDataType::Changed => {
                q.content.data.into_iter().map(T::into_ws).collect()
            },
            EdgexWsData::Trades(_) => Vec::new(),
            EdgexWsData::Event(EdgexWsEvent::Error { request, content }) => {
                warn!(
                    "edgeX WS error. code = {}, msg = {}, request = {:?}",
                    content.code, content.msg, request
                );
                Vec::new()
            },
            EdgexWsData::Event(EdgexWsEvent::Ping) => Vec::new(),
            EdgexWsData::Event(EdgexWsEvent::Connected { sid }) => {
                info!("edgeX WS connected. sid = {}", sid);
                Vec::new()
            },
            EdgexWsData::Event(
                EdgexWsEvent::Subscribed { channel } | EdgexWsEvent::Unsubscribed { channel },
            ) => {
                info!("edgeX WS subscription update. channel = {}", channel);
                Vec::new()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct TestPayload {
        value: u64,
    }

    impl IntoWsData for TestPayload {
        type Output = u64;

        fn into_ws(self) -> u64 {
            self.value
        }
    }

    fn quote(data_type: &str) -> String {
        format!(
            r#"{{"type":"quote-event","channel":"trades.30000001","content":{{
            "channel":"trades.30000001","dataType":"{data_type}","data":[{{"value":1}},{{"value":2}}]}}}}"#
        )
    }

    #[test]
    fn single_frames_become_one_event() {
        let data = EdgexWsData::<TestPayload>::decode_single(br#"{"value":3}"#).unwrap();

        assert_eq!(data.into_ws(), vec![3]);
    }

    #[test]
    fn batches_emit_snapshot_and_changed_entries() {
        for data_type in ["Snapshot", "changed", "Changed"] {
            let data =
                EdgexWsData::<TestPayload>::decode_batch(quote(data_type).as_bytes()).unwrap();
            assert!(matches!(data, EdgexWsData::ChannelBatch(_)));
            assert_eq!(data.into_ws(), vec![1, 2], "{data_type}");
        }
    }

    #[test]
    fn trade_history_snapshot_is_not_emitted() {
        let live = EdgexWsData::<TestPayload>::decode_trades(quote("changed").as_bytes()).unwrap();
        let history =
            EdgexWsData::<TestPayload>::decode_trades(quote("Snapshot").as_bytes()).unwrap();

        assert_eq!(live.into_ws(), vec![1, 2]);
        assert!(matches!(history, EdgexWsData::Trades(_)));
        assert!(history.into_ws().is_empty());
    }

    #[test]
    fn control_and_error_frames_become_no_events() {
        let frames: [&[u8]; 5] = [
            br#"{"sid":"f441d87c-7211-1024-0e2d-5997679ab1a1","type":"connected"}"#,
            br#"{"type":"subscribed","channel":"depth.30000001.15","request":"{\"type\":\"subscribe\",\"channel\":\"depth.30000001.15\"}"}"#,
            br#"{"type":"unsubscribed","channel":"depth.30000001.15"}"#,
            br#"{"type":"ping","time":"1790585480000"}"#,
            br#"{"type":"error","request":"{\"type\":\"subscribe\",\"channel\":\"depth.99999999.15\"}","content":{"code":"GATEWAY_INVALID_CONTRACT_ID","msg":"invalid contractId : 99999999"}}"#,
        ];

        for frame in frames {
            for data in [
                EdgexWsData::<TestPayload>::decode_single(frame).unwrap(),
                EdgexWsData::<TestPayload>::decode_batch(frame).unwrap(),
                EdgexWsData::<TestPayload>::decode_trades(frame).unwrap(),
            ] {
                assert!(matches!(data, EdgexWsData::Event(_)), "{frame:?}");
                assert!(data.into_ws().is_empty());
            }
        }
    }

    #[test]
    fn unknown_frames_fail_to_decode() {
        let frames: [&[u8]; 5] = [
            br#"{"type":"pong","time":"1"}"#,
            br#"{"type":"quote-event","channel":"x","content":{"dataType":"Delta","data":[]}}"#,
            br#"{"type":"notice","channel":"x","content":{"dataType":"changed","data":[]}}"#,
            br#"{"type":"error","content":{"code":"X"}}"#,
            br#"{"channel":"x"}"#,
        ];

        for frame in frames {
            assert!(
                EdgexWsData::<TestPayload>::decode_batch(frame).is_err(),
                "{frame:?}"
            );
        }
    }
}
