use serde::{Deserialize, de::DeserializeOwned};
use tracing::{debug, info, warn};

use extrema_infra::prelude::IntoWsData;

use crate::exchange::ws_decode::decode_preferred;

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum ApexWsData<T> {
    ChannelSingle(T),
    ChannelBatch(ApexWsBatch<T>),
    Event(ApexWsEvent),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApexFrameKind {
    Snapshot,
    Delta,
}

/// `recentlyTrade` replays the last 50 trades, newest first, as a snapshot; deltas carry new fills in order.
#[derive(Clone, Debug, Deserialize)]
pub struct ApexWsBatch<T> {
    #[serde(rename = "type")]
    pub kind: ApexFrameKind,
    pub data: Vec<T>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum ApexWsEvent {
    Response {
        success: bool,
        ret_msg: String,
        conn_id: String,
        request: ApexWsRequest,
    },
    Heartbeat {
        op: ApexWsOp,
        args: Vec<String>,
    },
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApexWsRequest {
    pub op: String,
    #[serde(default)]
    pub args: Option<Vec<String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApexWsOp {
    Ping,
    Pong,
}

impl<T: DeserializeOwned> ApexWsData<T> {
    pub(crate) fn decode_single(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::ChannelSingle)
    }

    pub(crate) fn decode_batch(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::ChannelBatch)
    }
}

impl<T> IntoWsData for ApexWsData<T>
where
    T: IntoWsData + for<'de> Deserialize<'de>,
{
    type Output = Vec<T::Output>;

    fn into_ws(self) -> Self::Output {
        match self {
            ApexWsData::ChannelSingle(c) => vec![c.into_ws()],
            ApexWsData::ChannelBatch(b) if b.kind == ApexFrameKind::Delta => {
                b.data.into_iter().map(|item| item.into_ws()).collect()
            },
            ApexWsData::ChannelBatch(_) => Vec::new(),
            ApexWsData::Event(ApexWsEvent::Response {
                success: false,
                ret_msg,
                conn_id,
                request,
            }) => {
                warn!(
                    "ApeX WS error. ret_msg = {}, op = {}, args = {:?}, conn_id = {}",
                    ret_msg, request.op, request.args, conn_id
                );
                Vec::new()
            },
            ApexWsData::Event(ApexWsEvent::Response {
                ret_msg, request, ..
            }) => {
                if request.op != "ping" {
                    info!(
                        "ApeX WS subscription update. op = {}, args = {:?}, ret_msg = {}",
                        request.op, request.args, ret_msg
                    );
                }
                Vec::new()
            },
            ApexWsData::Event(ApexWsEvent::Heartbeat { op, args }) => {
                debug!("ApeX WS heartbeat. op = {:?}, args = {:?}", op, args);
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

    #[test]
    fn single_frames_become_one_event() {
        let data = ApexWsData::<TestPayload>::decode_single(br#"{"value":6}"#).unwrap();

        assert_eq!(data.into_ws(), vec![6]);
    }

    #[test]
    fn delta_batches_become_events_in_frame_order() {
        let data = ApexWsData::<TestPayload>::decode_batch(
            br#"{"topic":"recentlyTrade.H.BTCUSDT","type":"delta","data":[{"value":1},{"value":2}],
            "cs":66026503066,"ts":1790586423571534}"#,
        )
        .unwrap();

        assert_eq!(data.into_ws(), vec![1, 2]);
    }

    #[test]
    fn subscribe_history_is_not_emitted() {
        let data = ApexWsData::<TestPayload>::decode_batch(
            br#"{"topic":"recentlyTrade.H.BTCUSDT","type":"snapshot","data":[{"value":1}],
            "cs":66026496011,"ts":1790586413372944}"#,
        )
        .unwrap();

        assert!(matches!(data, ApexWsData::ChannelBatch(_)));
        assert!(data.into_ws().is_empty());
    }

    #[test]
    fn control_frames_become_no_events() {
        let frames: [&[u8]; 7] = [
            br#"{"success":true,"ret_msg":"","conn_id":"0b7a676f-e0ad-4e76-90a2-4e957e559598","request":{"op":"subscribe","args":["orderBook200.H.BTCUSDT"]}}"#,
            br#"{"success":false,"ret_msg":"error:handler not found","conn_id":"6700e2c7-8b7f-48d1-9ce5-963a5bb71055","request":{"op":"subscribe","args":["orderBook25.H.NOPEUSDT"]}}"#,
            br#"{"success":false,"ret_msg":"error:topic:already subscribed recentlyTrade.H.NVDAUSDT","conn_id":"4eb43950-0f75-478f-9fef-8142d93e1345","request":{"op":"subscribe","args":["recentlyTrade.H.NVDAUSDT"]}}"#,
            br#"{"success":false,"ret_msg":"error:invalid op","conn_id":"4eb43950-0f75-478f-9fef-8142d93e1345","request":{"op":"","args":null}}"#,
            br#"{"success":true,"ret_msg":"pong","conn_id":"4eb43950-0f75-478f-9fef-8142d93e1345","request":{"op":"ping","args":["1790586782444"]}}"#,
            br#"{"success":true,"ret_msg":"","conn_id":"4eb43950-0f75-478f-9fef-8142d93e1345","request":{"op":"unsubscribe","args":["recentlyTrade.H.NVDAUSDT"]}}"#,
            br#"{"op":"ping","args":["1790586413937"]}"#,
        ];

        for frame in frames {
            let data = ApexWsData::<TestPayload>::decode_single(frame).unwrap();
            assert!(matches!(data, ApexWsData::Event(_)), "{frame:?}");
            assert!(data.into_ws().is_empty());
        }
    }

    #[test]
    fn unknown_frames_fail_to_decode() {
        for frame in [
            br#"{"topic":"instrumentInfo.H.BTCUSDT","type":"delta","data":{"symbol":"BTCUSDT"}}"#
                .as_slice(),
            br#"{"op":"subscribe","args":["orderBook25.H.BTCUSDT"]}"#,
            br#"{"success":true,"ret_msg":""}"#,
        ] {
            assert!(
                ApexWsData::<TestPayload>::decode_single(frame).is_err(),
                "{frame:?}"
            );
        }
    }
}
