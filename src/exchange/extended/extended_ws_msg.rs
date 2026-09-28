use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use tracing::warn;

use extrema_infra::prelude::IntoWsData;

use crate::exchange::ws_decode::decode_preferred;

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum ExtendedWsData<T> {
    ChannelSingle(T),
    Trades(ExtendedWsTrades<T>),
    Event(ExtendedWsEvent),
}

/// `publicTrades/{market}` frames; the first one (`seq` 1) replays the market's last 50 trades, however old.
#[derive(Clone, Debug, Deserialize)]
pub struct ExtendedWsTrades<T> {
    pub data: Vec<T>,
    pub seq: u64,
}

/// Error envelope, shaped like REST errors. Public streams otherwise send only data:
/// an unknown market streams nothing, and bad query parameters fail the handshake.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "status", rename_all = "UPPERCASE")]
pub enum ExtendedWsEvent {
    Error { error: ExtendedWsError },
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExtendedWsError {
    pub code: Value,
    pub message: String,
}

impl<T: DeserializeOwned> ExtendedWsData<T> {
    pub(crate) fn decode_single(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::ChannelSingle)
    }

    pub(crate) fn decode_trades(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::Trades)
    }
}

impl<T> IntoWsData for ExtendedWsData<T>
where
    T: IntoWsData + for<'de> Deserialize<'de>,
{
    type Output = Vec<T::Output>;

    fn into_ws(self) -> Self::Output {
        match self {
            ExtendedWsData::ChannelSingle(c) => vec![c.into_ws()],
            // The first frame replays trade history; only live prints become events.
            ExtendedWsData::Trades(t) if t.seq > 1 => {
                t.data.into_iter().map(|trade| trade.into_ws()).collect()
            },
            ExtendedWsData::Trades(_) => Vec::new(),
            ExtendedWsData::Event(ExtendedWsEvent::Error { error }) => {
                warn!(
                    "Extended WS error. code = {}, message = {}",
                    error.code, error.message
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
        let data = ExtendedWsData::<TestPayload>::decode_single(br#"{"value":3}"#).unwrap();

        assert_eq!(data.into_ws(), vec![3]);
    }

    #[test]
    fn live_trade_batches_become_events() {
        let data = ExtendedWsData::<TestPayload>::decode_trades(
            br#"{"data":[{"value":1},{"value":2}],"ts":1790586586457,"seq":6}"#,
        )
        .unwrap();

        assert_eq!(data.into_ws(), vec![1, 2]);
    }

    #[test]
    fn first_trade_frame_is_history() {
        for frame in [
            &br#"{"data":[{"value":1}],"ts":1790586554111,"seq":1}"#[..],
            br#"{"data":[],"ts":1790586554093,"seq":1}"#,
        ] {
            let data = ExtendedWsData::<TestPayload>::decode_trades(frame).unwrap();
            assert!(matches!(data, ExtendedWsData::Trades(_)));
            assert!(data.into_ws().is_empty());
        }
    }

    #[test]
    fn error_frames_become_no_events() {
        let data = ExtendedWsData::<TestPayload>::decode_single(
            br#"{"status":"ERROR","error":{"code":1001,"message":"Market not found"}}"#,
        )
        .unwrap();

        assert!(matches!(data, ExtendedWsData::Event(_)));
        assert!(data.into_ws().is_empty());
    }

    #[test]
    fn unknown_frames_fail_to_decode() {
        for frame in [
            &br#"{"type":"MP","data":{"m":"BTC-USD","p":"82603.9","ts":0},"ts":1790586555172,"seq":1}"#[..],
            br#"{"status":"OK"}"#,
            br#"{"data":[{"value":1}],"ts":1}"#,
        ] {
            assert!(
                ExtendedWsData::<TestPayload>::decode_trades(frame).is_err(),
                "{frame:?}"
            );
        }
    }
}
