use serde::{Deserialize, de::DeserializeOwned};
use tracing::{info, warn};

use extrema_infra::prelude::IntoWsData;

use crate::exchange::ws_decode::decode_preferred;

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum ArcusWsData<T> {
    ChannelSingle(T),
    Trades(ArcusWsTrades<T>),
    Event(ArcusWsEvent),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArcusFrameKind {
    Subscribed,
    ChannelData,
}

/// One taker order's fills; the channel sends no snapshot.
#[derive(Clone, Debug, Deserialize)]
pub struct ArcusWsTrades<T> {
    #[serde(rename = "type")]
    pub kind: ArcusFrameKind,
    pub contents: Vec<T>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ArcusWsEvent {
    Connected {
        connection_id: String,
    },
    Error {
        message: String,
    },
    /// The server holds the subscription snapshot and retries it after `retryAfterMs`.
    Degraded {
        channel: String,
        #[serde(default)]
        id: Option<String>,
        reason: String,
        #[serde(rename = "retryAfterMs", default)]
        retry_after_ms: Option<u64>,
    },
    Unsubscribed {
        channel: String,
        #[serde(default)]
        id: Option<String>,
    },
    /// Acknowledgement of a subscription without a snapshot, such as `trades`.
    Subscribed {
        channel: String,
        #[serde(default)]
        id: Option<String>,
        #[serde(rename = "contents")]
        _contents: ArcusEmptyContents,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArcusEmptyContents {}

impl<T: DeserializeOwned> ArcusWsData<T> {
    pub(crate) fn decode_single(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::ChannelSingle)
    }

    pub(crate) fn decode_trades(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::Trades)
    }
}

impl<T> IntoWsData for ArcusWsData<T>
where
    T: IntoWsData + for<'de> Deserialize<'de>,
{
    type Output = Vec<T::Output>;

    fn into_ws(self) -> Self::Output {
        match self {
            ArcusWsData::ChannelSingle(c) => vec![c.into_ws()],
            ArcusWsData::Trades(t) if t.kind == ArcusFrameKind::ChannelData => t
                .contents
                .into_iter()
                .map(|trade| trade.into_ws())
                .collect(),
            ArcusWsData::Trades(_) => Vec::new(),
            ArcusWsData::Event(ArcusWsEvent::Error { message }) => {
                warn!("Arcus WS error: {}", message);
                Vec::new()
            },
            ArcusWsData::Event(ArcusWsEvent::Degraded {
                channel,
                id,
                reason,
                retry_after_ms,
            }) => {
                warn!(
                    "Arcus WS degraded. channel = {}, id = {:?}, reason = {}, retry_after_ms = {:?}",
                    channel, id, reason, retry_after_ms
                );
                Vec::new()
            },
            ArcusWsData::Event(ArcusWsEvent::Connected { connection_id }) => {
                info!("Arcus WS connected. connection_id = {}", connection_id);
                Vec::new()
            },
            ArcusWsData::Event(
                ArcusWsEvent::Subscribed { channel, id, .. }
                | ArcusWsEvent::Unsubscribed { channel, id },
            ) => {
                info!(
                    "Arcus WS subscription update. channel = {}, id = {:?}",
                    channel, id
                );
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
        let data = ArcusWsData::<TestPayload>::decode_single(br#"{"value":2}"#).unwrap();

        assert_eq!(data.into_ws(), vec![2]);
    }

    #[test]
    fn live_trade_batches_become_events() {
        let data = ArcusWsData::<TestPayload>::decode_trades(
            br#"{"type":"channel_data","channel":"trades","id":"BTC-USD","contents":[{"value":1},{"value":2}]}"#,
        )
        .unwrap();

        assert_eq!(data.into_ws(), vec![1, 2]);
    }

    #[test]
    fn control_frames_become_no_events() {
        let frames: [&[u8]; 5] = [
            br#"{"type":"connected","connection_id":"1790577907185569311"}"#,
            br#"{"type":"error","message":"Invalid market 'NOPE-USD'."}"#,
            br#"{"type":"degraded","channel":"l2OrderbookUpdates","id":"BTC-USD","reason":"snapshot_stale","retryAfterMs":5000}"#,
            br#"{"type":"subscribed","channel":"trades","id":"NVDA-USD","contents":{}}"#,
            br#"{"type":"unsubscribed","channel":"trades","id":"NVDA-USD"}"#,
        ];

        for frame in frames {
            let data = ArcusWsData::<TestPayload>::decode_trades(frame).unwrap();
            assert!(matches!(data, ArcusWsData::Event(_)), "{frame:?}");
            assert!(data.into_ws().is_empty());
        }
    }

    #[test]
    fn subscribe_reply_with_contents_is_not_an_ack() {
        assert!(
            ArcusWsData::<TestPayload>::decode_single(
                br#"{"type":"subscribed","channel":"l2Orderbook","id":"NVDA-USD","contents":{"bids":[]}}"#
            )
            .is_err()
        );
    }
}
