use serde::{Deserialize, de::DeserializeOwned};
use tracing::{info, warn};

use extrema_infra::{
    arch::market_assets::api_general::de_u64_from_string_or_number, prelude::IntoWsData,
};

use crate::exchange::ws_decode::decode_preferred;

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum GrvtWsData<T> {
    ChannelSingle(T),
    Trade(GrvtWsFeed<T>),
    Event(GrvtWsEvent),
}

/// A `v1.trade` frame carries one fill; the subscribe reply first replays
/// recent fills with `sequence_number` 0.
#[derive(Clone, Debug, Deserialize)]
pub struct GrvtWsFeed<T> {
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    pub sequence_number: u64,
    pub feed: T,
}

/// JSON-RPC replies to `subscribe` / `unsubscribe`.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum GrvtWsEvent {
    Error {
        method: String,
        #[serde(default)]
        id: Option<u64>,
        error: GrvtWsError,
    },
    Result {
        method: String,
        #[serde(default)]
        id: Option<u64>,
        result: GrvtWsSubResult,
    },
}

#[derive(Clone, Debug, Deserialize)]
pub struct GrvtWsError {
    pub code: i64,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GrvtWsSubResult {
    pub stream: String,
    #[serde(default)]
    pub subs: Vec<String>,
    #[serde(default)]
    pub unsubs: Vec<String>,
    #[serde(default)]
    pub first_sequence_number: Vec<String>,
}

impl<T: DeserializeOwned> GrvtWsData<T> {
    pub(crate) fn decode_single(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::ChannelSingle)
    }

    pub(crate) fn decode_trades(frame: &[u8]) -> serde_json::Result<Self> {
        decode_preferred(frame, Self::Trade)
    }
}

impl<T> IntoWsData for GrvtWsData<T>
where
    T: IntoWsData + for<'de> Deserialize<'de>,
{
    type Output = Vec<T::Output>;

    fn into_ws(self) -> Self::Output {
        match self {
            GrvtWsData::ChannelSingle(c) => vec![c.into_ws()],
            GrvtWsData::Trade(t) if t.sequence_number > 0 => vec![t.feed.into_ws()],
            GrvtWsData::Trade(_) => Vec::new(),
            GrvtWsData::Event(GrvtWsEvent::Error { method, id, error }) => {
                warn!(
                    "GRVT WS error. code = {}, message = {}, method = {}, id = {:?}",
                    error.code, error.message, method, id
                );
                Vec::new()
            },
            GrvtWsData::Event(GrvtWsEvent::Result { method, id, result }) => {
                info!(
                    "GRVT WS {} reply. stream = {}, subs = {:?}, unsubs = {:?}, first_sequence_number = {:?}, id = {:?}",
                    method,
                    result.stream,
                    result.subs,
                    result.unsubs,
                    result.first_sequence_number,
                    id
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
        let data = GrvtWsData::<TestPayload>::decode_single(br#"{"value":3}"#).unwrap();

        assert_eq!(data.into_ws(), vec![3]);
    }

    #[test]
    fn live_fills_become_events_and_replayed_fills_do_not() {
        let live = GrvtWsData::<TestPayload>::decode_trades(
            br#"{"stream":"v1.trade","selector":"ETH_USDT_Perp@50","sequence_number":"9599","feed":{"value":7},"prev_sequence_number":"9598"}"#,
        )
        .unwrap();
        let replayed = GrvtWsData::<TestPayload>::decode_trades(
            br#"{"stream":"v1.trade","selector":"ETH_USDT_Perp@50","sequence_number":"0","feed":{"value":8},"prev_sequence_number":"0"}"#,
        )
        .unwrap();

        assert_eq!(live.into_ws(), vec![7]);
        assert!(matches!(replayed, GrvtWsData::Trade(_)));
        assert!(replayed.into_ws().is_empty());
    }

    #[test]
    fn json_rpc_replies_become_no_events() {
        let frames: [&[u8]; 5] = [
            br#"{"jsonrpc":"2.0","result":{"stream":"v1.book.d","subs":["BTC_USDT_Perp@50"],"unsubs":[],"num_snapshots":[1],"first_sequence_number":["36481"],"latest_sequence_number":["36480"]},"id":1,"method":"subscribe"}"#,
            br#"{"jsonrpc":"2.0","result":{"stream":"v1.mini.s","unsubs":["ETH_USDT_Perp@1000"]},"id":7,"method":"unsubscribe"}"#,
            br#"{"jsonrpc":"2.0","error":{"code":3000,"message":"Instrument is invalid"},"id":1,"method":"subscribe"}"#,
            br#"{"jsonrpc":"2.0","error":{"code":1003,"message":"Request could not be processed due to malformed syntax"},"id":0,"method":""}"#,
            br#"{"jsonrpc":"2.0","error":{"code":1200,"message":"RPC method not found"},"id":9,"method":"nope"}"#,
        ];

        for frame in frames {
            let data = GrvtWsData::<TestPayload>::decode_single(frame).unwrap();
            assert!(matches!(data, GrvtWsData::Event(_)), "{frame:?}");
            assert!(data.into_ws().is_empty());
        }
    }

    #[test]
    fn unknown_frames_fail_to_decode() {
        let frames: [&[u8]; 4] = [
            br#"{"code":1002,"message":"Internal Server Error","status":500}"#,
            br#"{"jsonrpc":"2.0","id":1,"method":"subscribe"}"#,
            br#"{"stream":"v1.mini.d","selector":"BTC_USDT_Perp@0","sequence_number":"1849216","feed":{"best_ask_size":"3.91"}}"#,
            br#"{"stream":"v1.trade","sequence_number":"x","feed":{"value":1}}"#,
        ];

        for frame in frames {
            assert!(
                GrvtWsData::<TestPayload>::decode_trades(frame).is_err(),
                "{frame:?}"
            );
        }
    }
}
