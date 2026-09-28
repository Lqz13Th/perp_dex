use serde::{Deserialize, Deserializer, de::DeserializeOwned, de::Error as DeError};
use serde_json::Value;
use tracing::warn;

use extrema_infra::prelude::{InfraError, InfraResult, IntoInfraData};

use crate::exchange::api_general::exactly_one;

const LIGHTER_OK: i64 = 200;

/// Lighter replies `{"code":200, ...payload}` or `{"code":<error>, "message":...}`.
#[derive(Clone, Debug)]
pub enum RestResLighter<T> {
    CodeMsg(LighterCodeMsg),
    Object(T),
}

#[derive(Clone, Debug, Deserialize)]
pub struct LighterCodeMsg {
    pub code: i64,
    #[serde(default)]
    pub message: String,
}

impl<T> IntoInfraData<T> for RestResLighter<T> {
    fn into_one(self) -> InfraResult<T> {
        match self {
            Self::Object(o) => Ok(o),
            other => exactly_one(other.into_vec()?),
        }
    }

    fn into_vec(self) -> InfraResult<Vec<T>> {
        match self {
            Self::Object(o) => Ok(vec![o]),
            Self::CodeMsg(LighterCodeMsg { code, message }) => {
                warn!("Lighter REST error {}: {}", code, message);
                Err(InfraError::ApiCliError(format!(
                    "Lighter REST error (code={}): {}",
                    code, message
                )))
            },
        }
    }
}

impl<'de, T> Deserialize<'de> for RestResLighter<T>
where
    T: DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;

        match value.get("code").and_then(Value::as_i64) {
            Some(code) if code != LIGHTER_OK => serde_json::from_value(value)
                .map(Self::CodeMsg)
                .map_err(D::Error::custom),
            _ => serde_json::from_value(value)
                .map(Self::Object)
                .map_err(D::Error::custom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct Payload {
        total_asks: u64,
    }

    #[test]
    fn ok_code_is_the_payload() {
        let res: RestResLighter<Payload> =
            serde_json::from_str(r#"{"code":200,"total_asks":5}"#).unwrap();

        assert_eq!(res.into_one().unwrap().total_asks, 5);
    }

    #[test]
    fn error_codes_are_errors() {
        for raw in [
            r#"{"code":21602,"message":"invalid market index"}"#,
            r#"{"code":20001,"message":"invalid param "}"#,
        ] {
            let res: RestResLighter<Payload> = serde_json::from_str(raw).unwrap();
            assert!(matches!(res, RestResLighter::CodeMsg(_)));
            let err = res.into_vec().unwrap_err().to_string();
            assert!(err.contains("Lighter REST error"), "{err}");
        }
    }

    #[test]
    fn payload_without_code_still_parses() {
        let res: RestResLighter<Payload> = serde_json::from_str(r#"{"total_asks":1}"#).unwrap();

        assert_eq!(res.into_one().unwrap().total_asks, 1);
    }

    #[test]
    fn malformed_payload_is_a_parse_error() {
        assert!(serde_json::from_str::<RestResLighter<Payload>>(r#"{"code":200}"#).is_err());
    }
}
