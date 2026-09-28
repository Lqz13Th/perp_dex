use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned, de::Error as DeError};
use serde_json::Value;
use tracing::warn;

use extrema_infra::arch::traits::conversion::IntoInfraData;
use extrema_infra::prelude::{InfraError, InfraResult};

use crate::exchange::api_general::exactly_one;

#[derive(Clone, Debug, Serialize)]
pub enum RestResAster<T> {
    CodeMsg(AsterCodeMsg),
    Data(Vec<T>),
    Object(T),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AsterCodeMsg {
    pub code: i64,
    pub msg: String,
}

impl<T> IntoInfraData<T> for RestResAster<T> {
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
            Self::CodeMsg(AsterCodeMsg { code, msg }) => {
                warn!("Aster REST error {}: {}", code, msg);
                Err(InfraError::ApiCliError(format!(
                    "Aster REST error (code={}): {}",
                    code, msg
                )))
            },
        }
    }
}

impl<'de, T> Deserialize<'de> for RestResAster<T>
where
    T: DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;

        match value {
            Value::Array(_) => serde_json::from_value(value)
                .map(Self::Data)
                .map_err(D::Error::custom),
            Value::Object(_) => {
                let code_msg = value
                    .get("code")
                    .and_then(Value::as_i64)
                    .zip(value.get("msg").and_then(Value::as_str));

                if let Some((code, _)) = code_msg
                    && code != 200
                {
                    return serde_json::from_value(value)
                        .map(Self::CodeMsg)
                        .map_err(D::Error::custom);
                }

                serde_json::from_value(value)
                    .map(Self::Object)
                    .map_err(D::Error::custom)
            },
            other => serde_json::from_value(other)
                .map(Self::Object)
                .map_err(D::Error::custom),
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
        let list: RestResAster<Ticker> =
            serde_json::from_str(r#"[{"symbol":"NVDAUSDT"},{"symbol":"MUUSD1"}]"#).unwrap();
        let one: RestResAster<Ticker> = serde_json::from_str(r#"{"symbol":"NVDAUSDT"}"#).unwrap();

        assert_eq!(list.into_vec().unwrap().len(), 2);
        assert_eq!(one.into_one().unwrap().symbol, "NVDAUSDT");
    }

    #[test]
    fn code_msg_payload_is_an_error() {
        let res: RestResAster<Value> =
            serde_json::from_str(r#"{"code":-1121,"msg":"Invalid symbol."}"#).unwrap();

        assert!(matches!(
            res,
            RestResAster::CodeMsg(AsterCodeMsg { code: -1121, .. })
        ));
        let err = res.into_vec().unwrap_err().to_string();
        assert!(err.contains("-1121") && err.contains("Invalid symbol."));
    }

    #[test]
    fn success_code_msg_is_data() {
        let res: RestResAster<AsterCodeMsg> =
            serde_json::from_str(r#"{"code":200,"msg":"success"}"#).unwrap();

        assert_eq!(res.into_one().unwrap().msg, "success");
    }

    #[test]
    fn into_one_rejects_lists_that_are_not_single() {
        let empty: RestResAster<Ticker> = serde_json::from_str("[]").unwrap();
        let several: RestResAster<Ticker> =
            serde_json::from_str(r#"[{"symbol":"A"},{"symbol":"B"}]"#).unwrap();

        assert!(empty.into_one().is_err());
        assert!(several.into_one().is_err());
    }
}
