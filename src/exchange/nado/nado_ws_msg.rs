use serde::{Deserialize, de::DeserializeOwned};
use tracing::{info, warn};

use extrema_infra::prelude::IntoWsData;

use crate::exchange::ws_decode::decode_preferred;

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum NadoWsData<T> {
    ChannelSingle(T),
    Event(NadoWsEvent),
}

/// Replies to a subscribe request; every stream event is its own frame.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum NadoWsEvent {
    Error(NadoWsError),
    Subscribed(NadoWsAck),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NadoWsError {
    #[serde(rename = "result")]
    _result: (),
    pub error: String,
    pub id: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NadoWsAck {
    #[serde(rename = "result")]
    _result: (),
    pub id: u64,
}

impl<T: DeserializeOwned> NadoWsData<T> {
    pub(crate) fn decode_single(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::ChannelSingle)
    }
}

impl<T> IntoWsData for NadoWsData<T>
where
    T: IntoWsData + for<'de> Deserialize<'de>,
{
    type Output = Vec<T::Output>;

    fn into_ws(self) -> Self::Output {
        match self {
            NadoWsData::ChannelSingle(c) => vec![c.into_ws()],
            NadoWsData::Event(NadoWsEvent::Error(err)) => {
                warn!("Nado WS error. id = {}, error = {}", err.id, err.error);
                Vec::new()
            },
            NadoWsData::Event(NadoWsEvent::Subscribed(ack)) => {
                info!("Nado WS subscribed. id = {}", ack.id);
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
        let data = NadoWsData::<TestPayload>::decode_single(br#"{"value":3}"#).unwrap();

        assert_eq!(data.into_ws(), vec![3]);
    }

    #[test]
    fn subscribe_ack_becomes_no_events() {
        let data = NadoWsData::<TestPayload>::decode_single(br#"{"result":null,"id":1}"#).unwrap();

        assert!(matches!(
            data,
            NadoWsData::Event(NadoWsEvent::Subscribed(NadoWsAck { id: 1, .. }))
        ));
        assert!(data.into_ws().is_empty());
    }

    #[test]
    fn subscribe_errors_become_no_events() {
        let frames: [&[u8]; 3] = [
            br#"{"result":null,"error":"The provided 'product_id' is invalid. Please verify and input a valid 'product_id'.","id":1}"#,
            br#"{"result":null,"error":"error parsing request: missing field `product_id`","id":1}"#,
            br#"{"result":null,"error":"error parsing request: missing field `id`","id":0}"#,
        ];

        for frame in frames {
            let data = NadoWsData::<TestPayload>::decode_single(frame).unwrap();
            assert!(
                matches!(data, NadoWsData::Event(NadoWsEvent::Error(_))),
                "{frame:?}"
            );
            assert!(data.into_ws().is_empty());
        }
    }

    #[test]
    fn unknown_frames_fail_to_decode() {
        let frames: [&[u8]; 5] = [
            br#"{"result":{"method":"pong","server_time":"1790586000000","client_time":"1"},"id":10}"#,
            br#"{"result":[{"type":"default"}],"id":10}"#,
            br#"{"result":null,"id":1,"extra":true}"#,
            br#"{"id":1}"#,
            br#"{"type":"funding_rate","timestamp":"1790586000000000000","product_id":2,"funding_rate_x18":"1","update_time":"1790586000"}"#,
        ];

        for frame in frames {
            assert!(
                NadoWsData::<TestPayload>::decode_single(frame).is_err(),
                "{frame:?}"
            );
        }
    }
}
