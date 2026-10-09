use serde::{Deserialize, de::DeserializeOwned};
use tracing::{info, warn};

use extrema_infra::prelude::IntoWsData;

use crate::exchange::ws_decode::decode_preferred;

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum LighterWsData<T> {
    ChannelSingle(T),
    Trades(LighterWsTrades<T>),
    Event(LighterWsEvent),
}

/// `trade/{id}` frames split one market's prints across `trades` and `liquidation_trades`.
#[derive(Clone, Debug, Deserialize)]
pub struct LighterWsTrades<T> {
    #[serde(rename = "type")]
    pub kind: String,
    pub trades: Vec<T>,
    #[serde(default = "Vec::new")]
    pub liquidation_trades: Vec<T>,
}

/// Account channel frame: every record it carries, flattened into one event.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum LighterWsAccountData<T> {
    Channel(T),
    Event(LighterWsEvent),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum LighterWsEvent {
    Error {
        error: LighterWsError,
    },
    Control {
        #[serde(rename = "type")]
        kind: LighterWsControl,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LighterWsControl {
    Connected,
    Pong,
    Unsubscribed,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LighterWsError {
    pub code: i64,
    pub message: String,
}

impl LighterWsEvent {
    fn log(self) {
        match self {
            LighterWsEvent::Error { error } => warn!(
                "Lighter WS error. code = {}, message = {}",
                error.code, error.message
            ),
            LighterWsEvent::Control { kind } => {
                if !matches!(kind, LighterWsControl::Pong) {
                    info!("Lighter WS event: {:?}", kind);
                }
            },
        }
    }
}

impl<T: DeserializeOwned> LighterWsAccountData<T> {
    pub(crate) fn decode(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::Channel)
    }
}

impl<T, O> IntoWsData for LighterWsAccountData<T>
where
    T: IntoWsData<Output = Vec<O>> + for<'de> Deserialize<'de>,
{
    type Output = Vec<O>;

    fn into_ws(self) -> Vec<O> {
        match self {
            LighterWsAccountData::Channel(c) => c.into_ws(),
            LighterWsAccountData::Event(e) => {
                e.log();
                Vec::new()
            },
        }
    }
}

impl<T: DeserializeOwned> LighterWsData<T> {
    pub(crate) fn decode_single(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::ChannelSingle)
    }

    pub(crate) fn decode_trades(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::Trades)
    }
}

impl<T> IntoWsData for LighterWsData<T>
where
    T: IntoWsData + for<'de> Deserialize<'de>,
{
    type Output = Vec<T::Output>;

    fn into_ws(self) -> Self::Output {
        match self {
            LighterWsData::ChannelSingle(c) => vec![c.into_ws()],
            // The subscribe reply replays recent history; only live prints become events.
            LighterWsData::Trades(t) if t.kind.starts_with("update/") => t
                .trades
                .into_iter()
                .chain(t.liquidation_trades)
                .map(|trade| trade.into_ws())
                .collect(),
            LighterWsData::Trades(_) => Vec::new(),
            LighterWsData::Event(e) => {
                e.log();
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
        let data = LighterWsData::<TestPayload>::decode_single(br#"{"value":4}"#).unwrap();

        assert_eq!(data.into_ws(), vec![4]);
    }

    #[test]
    fn live_trade_frames_emit_trades_then_liquidations() {
        let frame = br#"{"type":"update/trade","channel":"trade:1","nonce":1,
            "trades":[{"value":1},{"value":2}],"liquidation_trades":[{"value":3}]}"#;

        let data = LighterWsData::<TestPayload>::decode_trades(frame).unwrap();

        assert_eq!(data.into_ws(), vec![1, 2, 3]);
    }

    #[test]
    fn subscribe_trade_history_is_not_emitted() {
        let frame = br#"{"type":"subscribed/trade","channel":"trade:1","nonce":1,
            "trades":[{"value":1}],"liquidation_trades":[]}"#;

        let data = LighterWsData::<TestPayload>::decode_trades(frame).unwrap();

        assert!(matches!(data, LighterWsData::Trades(_)));
        assert!(data.into_ws().is_empty());
    }

    #[test]
    fn control_and_error_frames_become_no_events() {
        let frames: [&[u8]; 3] = [
            br#"{"session_id":"f75d","type":"connected"}"#,
            br#"{"type":"pong"}"#,
            br#"{"error":{"code":30005,"message":"Invalid Channel:  (marketId)"}}"#,
        ];

        for frame in frames {
            let data = LighterWsData::<TestPayload>::decode_single(frame).unwrap();
            assert!(matches!(data, LighterWsData::Event(_)), "{frame:?}");
            assert!(data.into_ws().is_empty());
        }
    }

    #[test]
    fn unknown_frames_fail_to_decode() {
        assert!(
            LighterWsData::<TestPayload>::decode_single(br#"{"type":"update/height"}"#).is_err()
        );
        assert!(LighterWsData::<TestPayload>::decode_single(br#"{"channel":"x"}"#).is_err());
    }
}
