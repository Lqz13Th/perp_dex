use serde::{Deserialize, Deserializer, de::DeserializeOwned, de::Error as DeError};
use serde_json::Value;
use tracing::warn;

use extrema_infra::prelude::{InfraError, InfraResult, IntoInfraData};

use crate::exchange::api_general::exactly_one;

/// ApeX replies `{"data": ..., "timeCost": ...}`, or `{"code": ..., "msg": ...}` on failure.
#[derive(Clone, Debug)]
pub enum RestResApex<T> {
    CodeMsg(ApexCodeMsg),
    Data(Vec<T>),
    Object(T),
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApexCodeMsg {
    pub code: i64,
    #[serde(default)]
    pub msg: String,
}

impl<T> IntoInfraData<T> for RestResApex<T> {
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
            Self::CodeMsg(ApexCodeMsg { code, msg }) => {
                warn!("ApeX REST error {}: {}", code, msg);
                Err(InfraError::ApiCliError(format!(
                    "ApeX REST error (code={}): {}",
                    code, msg
                )))
            },
        }
    }
}

impl<'de, T> Deserialize<'de> for RestResApex<T>
where
    T: DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut value = Value::deserialize(deserializer)?;

        if value
            .get("code")
            .is_some_and(|code| code.as_i64() != Some(0))
        {
            return serde_json::from_value(value)
                .map(Self::CodeMsg)
                .map_err(D::Error::custom);
        }

        match value.get_mut("data").map(Value::take) {
            Some(data @ Value::Array(_)) => serde_json::from_value(data)
                .map(Self::Data)
                .map_err(D::Error::custom),
            Some(data) => serde_json::from_value(data)
                .map(Self::Object)
                .map_err(D::Error::custom),
            None => Err(D::Error::missing_field("data")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, Deserialize, PartialEq)]
    struct Ticker {
        symbol: String,
    }

    #[test]
    fn parses_array_and_object_payloads() {
        let list: RestResApex<Ticker> = serde_json::from_str(
            r#"{"data":[{"symbol":"BTCUSDT"},{"symbol":"NVDAUSDT"}],"timeCost":1524}"#,
        )
        .unwrap();
        let one: RestResApex<Ticker> =
            serde_json::from_str(r#"{"data":{"symbol":"BTCUSDT"},"timeCost":999}"#).unwrap();

        assert_eq!(list.into_vec().unwrap().len(), 2);
        assert_eq!(one.into_one().unwrap().symbol, "BTCUSDT");
    }

    #[test]
    fn code_msg_payloads_are_errors() {
        for (raw, code, msg) in [
            (
                r#"{"code":2,"msg":"internal server error","timeCost":2012}"#,
                "code=2",
                "internal server error",
            ),
            (
                r#"{"code":404,"msg":"Page Not Found","data":""}"#,
                "code=404",
                "Page Not Found",
            ),
        ] {
            let res: RestResApex<Ticker> = serde_json::from_str(raw).unwrap();
            assert!(matches!(res, RestResApex::CodeMsg(_)));
            let err = res.into_vec().unwrap_err().to_string();
            assert!(err.contains(code) && err.contains(msg), "{err}");
        }
    }

    #[test]
    fn zero_code_is_data() {
        let res: RestResApex<Ticker> =
            serde_json::from_str(r#"{"code":0,"data":{"symbol":"BTCUSDT"}}"#).unwrap();

        assert_eq!(res.into_one().unwrap().symbol, "BTCUSDT");
    }

    #[test]
    fn into_one_rejects_lists_that_are_not_single() {
        let empty: RestResApex<Ticker> =
            serde_json::from_str(r#"{"data":[],"timeCost":898}"#).unwrap();
        let several: RestResApex<Ticker> =
            serde_json::from_str(r#"{"data":[{"symbol":"A"},{"symbol":"B"}]}"#).unwrap();

        assert!(empty.into_one().is_err());
        assert!(several.into_one().is_err());
    }

    #[test]
    fn missing_or_malformed_data_is_a_parse_error() {
        assert!(serde_json::from_str::<RestResApex<Ticker>>(r#"{"timeCost":1}"#).is_err());
        assert!(serde_json::from_str::<RestResApex<Ticker>>(r#"{"data":{"other":1}}"#).is_err());
    }
}
