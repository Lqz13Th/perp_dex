use serde::{Deserialize, Deserializer, de::DeserializeOwned, de::Error as DeError};
use serde_json::Value;
use tracing::warn;

use extrema_infra::prelude::{InfraError, InfraResult, IntoInfraData};

use crate::exchange::api_general::exactly_one;

/// Extended replies `{"status":"OK","data":...}` or `{"status":"ERROR","error":{...}}`.
///
/// An unknown market's orderbook is `{"status":"OK"}` without `data`.
#[derive(Clone, Debug)]
pub enum RestResExtended<T> {
    Error(ExtendedError),
    Data(Vec<T>),
    Object(T),
    Empty,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ExtendedError {
    pub code: Value,
    pub message: String,
}

#[derive(Deserialize)]
struct ExtendedErrorReply {
    error: ExtendedError,
}

impl<T> IntoInfraData<T> for RestResExtended<T> {
    fn into_one(self) -> InfraResult<T> {
        match self {
            Self::Object(o) => Ok(o),
            other => exactly_one(other.into_vec()?),
        }
    }

    fn into_vec(self) -> InfraResult<Vec<T>> {
        match self {
            Self::Data(v) => Ok(v),
            Self::Object(o) => Ok(vec![o]),
            Self::Empty => Err(InfraError::ApiCliError(
                "Extended REST reply has no data".into(),
            )),
            Self::Error(ExtendedError { code, message }) => {
                warn!("Extended REST error {}: {}", code, message);
                Err(InfraError::ApiCliError(format!(
                    "Extended REST error (code={}): {}",
                    code, message
                )))
            },
        }
    }
}

impl<'de, T> Deserialize<'de> for RestResExtended<T>
where
    T: DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut value = Value::deserialize(deserializer)?;

        match value.get("status").and_then(Value::as_str) {
            Some("ERROR") => serde_json::from_value::<ExtendedErrorReply>(value)
                .map(|reply| Self::Error(reply.error))
                .map_err(D::Error::custom),
            Some("OK") => match value.get_mut("data").map(Value::take) {
                None | Some(Value::Null) => Ok(Self::Empty),
                Some(data @ Value::Array(_)) => serde_json::from_value(data)
                    .map(Self::Data)
                    .map_err(D::Error::custom),
                Some(data) => serde_json::from_value(data)
                    .map(Self::Object)
                    .map_err(D::Error::custom),
            },
            status => Err(D::Error::custom(format!(
                "unexpected Extended REST status: {:?}",
                status
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct Market {
        name: String,
    }

    #[test]
    fn list_and_object_payloads_parse() {
        let list: RestResExtended<Market> = serde_json::from_str(
            r#"{"status":"OK","data":[{"name":"BTC-USD"},{"name":"NVDA_24_5-USD"}]}"#,
        )
        .unwrap();
        let one: RestResExtended<Market> =
            serde_json::from_str(r#"{"status":"OK","data":{"name":"BTC-USD"}}"#).unwrap();

        assert_eq!(list.into_vec().unwrap().len(), 2);
        assert_eq!(one.into_one().unwrap().name, "BTC-USD");
    }

    #[test]
    fn error_payload_is_an_error() {
        let res: RestResExtended<Market> = serde_json::from_str(
            r#"{"status":"ERROR","error":{"code":1001,"message":"Market not found"}}"#,
        )
        .unwrap();

        assert!(matches!(res, RestResExtended::Error(_)));
        let err = res.into_vec().unwrap_err().to_string();
        assert!(
            err.contains("1001") && err.contains("Market not found"),
            "{err}"
        );
    }

    #[test]
    fn ok_without_data_is_an_error() {
        let res: RestResExtended<Market> = serde_json::from_str(r#"{"status":"OK"}"#).unwrap();

        assert!(matches!(res, RestResExtended::Empty));
        assert!(res.into_one().is_err());
    }

    #[test]
    fn into_one_rejects_lists_that_are_not_single() {
        let empty: RestResExtended<Market> =
            serde_json::from_str(r#"{"status":"OK","data":[]}"#).unwrap();

        assert!(empty.into_one().is_err());
    }

    #[test]
    fn malformed_payload_is_a_parse_error() {
        for raw in [
            r#"{"data":{"name":"BTC-USD"}}"#,
            r#"{"status":"OK","data":{"other":1}}"#,
            r#"{"status":"ERROR"}"#,
        ] {
            assert!(
                serde_json::from_str::<RestResExtended<Market>>(raw).is_err(),
                "{raw}"
            );
        }
    }
}
