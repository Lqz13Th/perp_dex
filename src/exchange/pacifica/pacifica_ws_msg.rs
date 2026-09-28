use serde::{Deserialize, de::DeserializeOwned};
use tracing::{info, warn};

use extrema_infra::prelude::IntoWsData;

use crate::exchange::ws_decode::decode_preferred;

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum PacificaWsData<T> {
    ChannelSingle(PacificaWsChannel<T>),
    ChannelBatch(PacificaWsChannel<Vec<T>>),
    Event(PacificaWsEvent),
}

/// `{"channel": <source>, "data": ...}`; `trades` carries a list, `book` and `bbo` one object.
#[derive(Clone, Debug, Deserialize)]
pub struct PacificaWsChannel<T> {
    #[serde(rename = "channel")]
    _channel: PacificaWsSource,
    pub data: T,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PacificaWsSource {
    Book,
    Bbo,
    Trades,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum PacificaWsEvent {
    /// A rejected subscription, e.g. an unknown symbol; the connection stays open.
    Error {
        error: String,
    },
    /// A malformed request.
    Rejected {
        code: i64,
        err: String,
    },
    Control(PacificaWsControl),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "channel", rename_all = "lowercase")]
pub enum PacificaWsControl {
    Subscribe { data: PacificaWsSubscription },
    Unsubscribe { data: PacificaWsSubscription },
    Pong,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PacificaWsSubscription {
    pub source: String,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub agg_level: Option<u16>,
}

impl<T: DeserializeOwned> PacificaWsData<T> {
    pub(crate) fn decode_single(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::ChannelSingle)
    }

    pub(crate) fn decode_batch(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::ChannelBatch)
    }
}

impl<T> IntoWsData for PacificaWsData<T>
where
    T: IntoWsData + for<'de> Deserialize<'de>,
{
    type Output = Vec<T::Output>;

    fn into_ws(self) -> Self::Output {
        match self {
            PacificaWsData::ChannelSingle(c) => vec![c.data.into_ws()],
            PacificaWsData::ChannelBatch(c) => c.data.into_iter().map(|d| d.into_ws()).collect(),
            PacificaWsData::Event(PacificaWsEvent::Error { error }) => {
                warn!("Pacifica WS error: {}", error);
                Vec::new()
            },
            PacificaWsData::Event(PacificaWsEvent::Rejected { code, err }) => {
                warn!("Pacifica WS error. code = {}, err = {}", code, err);
                Vec::new()
            },
            PacificaWsData::Event(PacificaWsEvent::Control(
                PacificaWsControl::Subscribe { data } | PacificaWsControl::Unsubscribe { data },
            )) => {
                info!(
                    "Pacifica WS subscription update. source = {}, symbol = {:?}, agg_level = {:?}",
                    data.source, data.symbol, data.agg_level
                );
                Vec::new()
            },
            PacificaWsData::Event(PacificaWsEvent::Control(PacificaWsControl::Pong)) => Vec::new(),
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
        let data = PacificaWsData::<TestPayload>::decode_single(
            br#"{"channel":"bbo","data":{"value":3}}"#,
        )
        .unwrap();

        assert_eq!(data.into_ws(), vec![3]);
    }

    #[test]
    fn batch_frames_become_one_event_each() {
        let data = PacificaWsData::<TestPayload>::decode_batch(
            br#"{"channel":"trades","data":[{"value":1},{"value":2}]}"#,
        )
        .unwrap();

        assert_eq!(data.into_ws(), vec![1, 2]);
    }

    #[test]
    fn control_and_error_frames_become_no_events() {
        let frames: [&[u8]; 6] = [
            br#"{"channel":"subscribe","data":{"source":"book","symbol":"BTC","agg_level":1}}"#,
            br#"{"channel":"subscribe","data":{"source":"trades","symbol":"SOL"}}"#,
            br#"{"channel":"unsubscribe","data":{"source":"bbo","symbol":"BTC"}}"#,
            br#"{"channel":"pong"}"#,
            br#"{"error":"Symbol not found"}"#,
            br#"{"code":400,"err":"Invalid subscription parameters.","t":1790585634106}"#,
        ];

        for frame in frames {
            let single = PacificaWsData::<TestPayload>::decode_single(frame).unwrap();
            let batch = PacificaWsData::<TestPayload>::decode_batch(frame).unwrap();
            assert!(matches!(single, PacificaWsData::Event(_)), "{frame:?}");
            assert!(matches!(batch, PacificaWsData::Event(_)), "{frame:?}");
            assert!(single.into_ws().is_empty() && batch.into_ws().is_empty());
        }
    }

    #[test]
    fn unknown_frames_fail_to_decode() {
        let frames: [&[u8]; 5] = [
            br#"{"channel":"prices","data":[{"value":1}]}"#,
            br#"{"channel":"candle","data":{"value":1}}"#,
            br#"{"channel":"ping"}"#,
            br#"{"code":200,"data":{"i":1},"id":"x","t":1,"type":"create_order"}"#,
            br#"{"channel":"bbo"}"#,
        ];

        for frame in frames {
            assert!(
                PacificaWsData::<TestPayload>::decode_single(frame).is_err(),
                "{frame:?}"
            );
        }
    }
}
