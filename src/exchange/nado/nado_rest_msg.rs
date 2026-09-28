use serde::{Deserialize, Deserializer, de::DeserializeOwned, de::Error as DeError};
use serde_json::Value;
use tracing::warn;

use extrema_infra::prelude::{InfraError, InfraResult, IntoInfraData};

use crate::exchange::api_general::exactly_one;

/// The gateway replies `{"status":"success","data":...}` or `{"status":"failure","error_code":..,"error":..}`;
/// the archive replies with the payload itself, or `{"error_code":..,"error":..}`.
#[derive(Clone, Debug)]
pub enum RestResNado<T> {
    Error(NadoError),
    Object(T),
}

#[derive(Clone, Debug, Deserialize)]
pub struct NadoError {
    #[serde(default)]
    pub error_code: i64,
    #[serde(default)]
    pub error: String,
}

impl<T> IntoInfraData<T> for RestResNado<T> {
    fn into_one(self) -> InfraResult<T> {
        match self {
            Self::Object(o) => Ok(o),
            other => exactly_one(other.into_vec()?),
        }
    }

    fn into_vec(self) -> InfraResult<Vec<T>> {
        match self {
            Self::Object(o) => Ok(vec![o]),
            Self::Error(NadoError { error_code, error }) => {
                warn!("Nado REST error {}: {}", error_code, error);
                Err(InfraError::ApiCliError(format!(
                    "Nado REST error (code={}): {}",
                    error_code, error
                )))
            },
        }
    }
}

impl<'de, T> Deserialize<'de> for RestResNado<T>
where
    T: DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let status = value.get("status").and_then(Value::as_str);

        if status == Some("failure") || value.get("error").is_some_and(Value::is_string) {
            return serde_json::from_value(value)
                .map(Self::Error)
                .map_err(D::Error::custom);
        }

        let payload = match value {
            Value::Object(mut map) if status == Some("success") => {
                map.remove("data").unwrap_or(Value::Null)
            },
            other => other,
        };

        serde_json::from_value(payload)
            .map(Self::Object)
            .map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[derive(Debug, Deserialize)]
    struct Price {
        product_id: u32,
        bid_x18: String,
    }

    #[derive(Debug, Deserialize)]
    struct Ticker {
        product_id: u32,
    }

    #[test]
    fn gateway_success_is_the_data() {
        let res: RestResNado<Price> = serde_json::from_str(
            r#"{"status":"success","data":{"product_id":2,"bid_x18":"82973000000000000000000",
            "ask_x18":"82974000000000000000000"},"request_type":"query_market_price"}"#,
        )
        .unwrap();

        let price = res.into_one().unwrap();
        assert_eq!(
            (price.product_id, price.bid_x18.as_str()),
            (2, "82973000000000000000000")
        );
    }

    #[test]
    fn gateway_failure_is_an_error() {
        let res: RestResNado<Price> = serde_json::from_str(
            r#"{"status":"failure","error_code":2015,"error":"The market for the given product or ticker ID was not found. Please try again with a different product or ticker ID.","request_type":"query_market_liquidity"}"#,
        )
        .unwrap();

        assert!(matches!(
            res,
            RestResNado::Error(NadoError {
                error_code: 2015,
                ..
            })
        ));
        let err = res.into_vec().unwrap_err().to_string();
        assert!(
            err.contains("code=2015") && err.contains("market for the given product"),
            "{err}"
        );
    }

    #[test]
    fn archive_payload_is_the_object() {
        let res: RestResNado<HashMap<String, Ticker>> = serde_json::from_str(
            r#"{"BTC-PERP_USDT0":{"product_id":2},"KBTC_USDT0":{"product_id":1}}"#,
        )
        .unwrap();

        let tickers = res.into_one().unwrap();
        assert_eq!(tickers["BTC-PERP_USDT0"].product_id, 2);
        assert_eq!(tickers.len(), 2);
    }

    #[test]
    fn archive_error_is_an_error() {
        let res: RestResNado<HashMap<String, Ticker>> = serde_json::from_str(
            r#"{"error":"The market for the given product or ticker ID was not found. Please try again with a different product or ticker ID.","error_code":2015}"#,
        )
        .unwrap();

        assert!(
            res.into_vec()
                .unwrap_err()
                .to_string()
                .contains("code=2015")
        );
    }

    #[test]
    fn failure_without_details_is_still_an_error() {
        let res: RestResNado<Price> = serde_json::from_str(r#"{"status":"failure"}"#).unwrap();

        assert!(res.into_one().is_err());
    }

    #[test]
    fn malformed_payload_is_a_parse_error() {
        assert!(serde_json::from_str::<RestResNado<Price>>(r#"{"status":"success"}"#).is_err());
        assert!(
            serde_json::from_str::<RestResNado<Price>>(r#"{"status":"success","data":{}}"#)
                .is_err()
        );
    }
}
