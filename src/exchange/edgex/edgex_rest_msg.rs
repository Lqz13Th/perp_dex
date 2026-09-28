use serde::{Deserialize, Deserializer, de::DeserializeOwned, de::Error as DeError};
use serde_json::Value;
use tracing::warn;

use extrema_infra::{
    arch::market_assets::api_general::ts_to_micros,
    prelude::{InfraError, InfraResult, IntoInfraData},
};

use crate::exchange::api_general::exactly_one;

const EDGEX_OK: &str = "SUCCESS";

/// edgeX replies `{"code":"SUCCESS","data":...}` or `{"code":<error>,"msg":...}`,
/// both stamped with the server's `responseTime`.
#[derive(Clone, Debug)]
pub enum RestResEdgex<T> {
    CodeMsg(EdgexCodeMsg),
    Object { data: T, response_time: u64 },
}

#[derive(Clone, Debug, Deserialize)]
pub struct EdgexCodeMsg {
    pub code: String,
    #[serde(default)]
    pub msg: Option<String>,
}

impl<T> RestResEdgex<T> {
    /// Server time of a successful reply in µs; edgeX quote payloads carry no time of their own.
    pub fn response_time(&self) -> u64 {
        match self {
            Self::Object { response_time, .. } => ts_to_micros(*response_time),
            Self::CodeMsg(_) => 0,
        }
    }
}

impl<T> IntoInfraData<T> for RestResEdgex<T> {
    fn into_one(self) -> InfraResult<T> {
        match self {
            Self::Object { data, .. } => Ok(data),
            other => exactly_one(other.into_vec()?),
        }
    }

    fn into_vec(self) -> InfraResult<Vec<T>> {
        match self {
            Self::Object { data, .. } => Ok(vec![data]),
            Self::CodeMsg(EdgexCodeMsg { code, msg }) => {
                let msg = msg.unwrap_or_default();
                warn!("edgeX REST error {}: {}", code, msg);
                Err(InfraError::ApiCliError(format!(
                    "edgeX REST error (code={}): {}",
                    code, msg
                )))
            },
        }
    }
}

impl<'de, T> Deserialize<'de> for RestResEdgex<T>
where
    T: DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut value = Value::deserialize(deserializer)?;

        if value.get("code").and_then(Value::as_str) != Some(EDGEX_OK) {
            return serde_json::from_value(value)
                .map(Self::CodeMsg)
                .map_err(D::Error::custom);
        }

        let response_time = value
            .get("responseTime")
            .and_then(Value::as_str)
            .and_then(|time| time.parse().ok())
            .unwrap_or_default();

        serde_json::from_value(value["data"].take())
            .map(|data| Self::Object {
                data,
                response_time,
            })
            .map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct Payload {
        level: u16,
    }

    #[test]
    fn success_is_the_data_stamped_with_the_server_time() {
        let res: RestResEdgex<Vec<Payload>> = serde_json::from_str(
            r#"{"code":"SUCCESS","data":[{"level":15}],"msg":null,"errorParam":null,
            "requestTime":"1790585440699","responseTime":"1790585440700","traceId":"d817b095b9a25b57"}"#,
        )
        .unwrap();

        assert_eq!(res.response_time(), 1_790_585_440_700_000);
        assert_eq!(res.into_one().unwrap()[0].level, 15);
    }

    #[test]
    fn error_codes_are_errors() {
        let res: RestResEdgex<Vec<Payload>> = serde_json::from_str(
            r#"{"code":"INVALID_DEPTH_LEVEL","data":null,"msg":"depth level only support 15 and 200",
            "errorParam":{},"requestTime":"1790585424919","responseTime":"1790585424921",
            "traceId":"901f66a11ae3b09125a68aa293f5aa05"}"#,
        )
        .unwrap();

        assert!(matches!(res, RestResEdgex::CodeMsg(_)));
        assert_eq!(res.response_time(), 0);
        let err = res.into_vec().unwrap_err().to_string();
        assert!(
            err.contains("INVALID_DEPTH_LEVEL") && err.contains("only support 15 and 200"),
            "{err}"
        );
    }

    #[test]
    fn empty_list_is_data() {
        let res: RestResEdgex<Vec<Payload>> = serde_json::from_str(
            r#"{"code":"SUCCESS","data":[],"msg":null,"responseTime":"1790585425147"}"#,
        )
        .unwrap();

        assert!(res.into_one().unwrap().is_empty());
    }

    #[test]
    fn non_envelope_and_malformed_payloads_are_parse_errors() {
        for raw in [
            r#"{"timestamp":"2026-09-28T08:50:41.873+00:00","status":404,"error":"Not Found","path":"/x"}"#,
            r#"{"code":"SUCCESS","data":null,"responseTime":"1"}"#,
            r#"{"code":"SUCCESS","data":[{"other":1}],"responseTime":"1"}"#,
        ] {
            assert!(
                serde_json::from_str::<RestResEdgex<Vec<Payload>>>(raw).is_err(),
                "{raw}"
            );
        }
    }
}
