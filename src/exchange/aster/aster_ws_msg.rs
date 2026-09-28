use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use tracing::{info, warn};

use extrema_infra::prelude::IntoWsData;

use crate::exchange::ws_decode::decode_preferred;

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum AsterWsData<T> {
    ChannelSingle(T),
    Event(AsterWsRes),
}

#[derive(Clone, Debug, Deserialize)]
pub struct AsterWsRes {
    pub result: Option<Value>,
    pub id: Value,
    pub error: Option<AsterWsError>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AsterWsError {
    pub code: i64,
    pub msg: String,
}

impl<T: DeserializeOwned> AsterWsData<T> {
    pub(crate) fn decode_single(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::ChannelSingle)
    }
}

impl<T> IntoWsData for AsterWsData<T>
where
    T: IntoWsData + for<'de> Deserialize<'de>,
{
    type Output = Vec<T::Output>;

    fn into_ws(self) -> Self::Output {
        match self {
            AsterWsData::ChannelSingle(c) => vec![c.into_ws()],
            AsterWsData::Event(res) => {
                if let Some(err) = &res.error {
                    warn!(
                        "Aster subscription error. code = {}, msg = {}, id = {}",
                        err.code, err.msg, res.id
                    );
                } else {
                    info!(
                        "Aster subscription received. result = {:?}, id = {}",
                        res.result, res.id
                    );
                }

                Vec::new()
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

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
    fn data_frames_become_one_event() {
        let data = AsterWsData::<TestPayload>::decode_single(br#"{"value":9}"#).unwrap();

        assert_eq!(data.into_ws(), vec![9]);
    }

    #[test]
    fn control_frames_become_no_events() {
        let frames: [&[u8]; 2] = [
            br#"{"id":1,"result":null}"#,
            br#"{"id":2,"error":{"code":2,"msg":"Invalid request"}}"#,
        ];

        for frame in frames {
            let data = AsterWsData::<TestPayload>::decode_single(frame).unwrap();
            assert!(matches!(data, AsterWsData::Event(_)));
            assert!(data.into_ws().is_empty());
        }
    }

    #[test]
    fn frames_of_another_stream_fail_to_decode() {
        assert!(AsterWsData::<TestPayload>::decode_single(br#"{"e":"markPriceUpdate"}"#).is_err());
    }
}
