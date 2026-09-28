use serde::{Deserialize, Deserializer, de::DeserializeOwned, de::Error as DeError};
use serde_json::Value;
use tracing::warn;

use extrema_infra::prelude::{InfraError, InfraResult, IntoInfraData};

use crate::exchange::api_general::exactly_one;

/// GRVT replies `{"result": ...}`, or `{"code", "message", "status"}` on failure.
#[derive(Clone, Debug)]
pub enum RestResGrvt<T> {
    CodeMsg(GrvtCodeMsg),
    Data(Vec<T>),
    Object(T),
}

#[derive(Clone, Debug, Deserialize)]
pub struct GrvtCodeMsg {
    pub code: i64,
    pub message: String,
    #[serde(default)]
    pub status: Option<u16>,
}

impl<T> IntoInfraData<T> for RestResGrvt<T> {
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
            Self::CodeMsg(GrvtCodeMsg {
                code,
                message,
                status,
            }) => {
                warn!(
                    "GRVT REST error {} (status={:?}): {}",
                    code, status, message
                );
                Err(InfraError::ApiCliError(format!(
                    "GRVT REST error (code={}): {}",
                    code, message
                )))
            },
        }
    }
}

impl<'de, T> Deserialize<'de> for RestResGrvt<T>
where
    T: DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut value = Value::deserialize(deserializer)?;

        match value.get_mut("result").map(Value::take) {
            Some(Value::Array(items)) => serde_json::from_value(Value::Array(items))
                .map(Self::Data)
                .map_err(D::Error::custom),
            Some(result) => serde_json::from_value(result)
                .map(Self::Object)
                .map_err(D::Error::custom),
            None => serde_json::from_value(value)
                .map(Self::CodeMsg)
                .map_err(D::Error::custom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, Deserialize, PartialEq)]
    struct Payload {
        instrument: String,
    }

    #[test]
    fn result_object_and_list_are_data() {
        let one: RestResGrvt<Payload> =
            serde_json::from_str(r#"{"result":{"instrument":"NVDA_USDT_Perp"}}"#).unwrap();
        let list: RestResGrvt<Payload> = serde_json::from_str(
            r#"{"result":[{"instrument":"AAOI_USDT_Perp"},{"instrument":"AAPL_USDT_Perp"}]}"#,
        )
        .unwrap();

        assert_eq!(one.into_one().unwrap().instrument, "NVDA_USDT_Perp");
        assert_eq!(list.into_vec().unwrap().len(), 2);
    }

    #[test]
    fn code_message_payloads_are_errors() {
        for (raw, code) in [
            (
                r#"{"code":1003,"message":"Request could not be processed due to malformed syntax","status":400}"#,
                "1003",
            ),
            (
                r#"{"code":3031,"message":"Depth is invalid","status":400}"#,
                "3031",
            ),
            (
                r#"{"code":1004,"message":"Data Not Found","status":404}"#,
                "1004",
            ),
        ] {
            let res: RestResGrvt<Payload> = serde_json::from_str(raw).unwrap();
            assert!(matches!(res, RestResGrvt::CodeMsg(_)));
            let err = res.into_vec().unwrap_err().to_string();
            assert!(
                err.contains("GRVT REST error") && err.contains(code),
                "{err}"
            );
        }
    }

    #[test]
    fn into_one_rejects_lists_that_are_not_single() {
        let empty: RestResGrvt<Payload> = serde_json::from_str(r#"{"result":[]}"#).unwrap();

        assert!(empty.into_one().is_err());
    }

    #[test]
    fn malformed_payloads_are_parse_errors() {
        assert!(serde_json::from_str::<RestResGrvt<Payload>>(r#"{"result":{"other":1}}"#).is_err());
        assert!(serde_json::from_str::<RestResGrvt<Payload>>(r#"{"other":1}"#).is_err());
    }
}
