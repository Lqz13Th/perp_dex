use serde::{Deserialize, Deserializer, de::DeserializeOwned, de::Error as DeError};
use serde_json::Value;
use tracing::warn;

use extrema_infra::prelude::{InfraError, InfraResult, IntoInfraData};

use crate::exchange::api_general::exactly_one;

/// Pacifica replies `{"success":true,"data":...}` or `{"success":false,"error":...,"code":...}`.
#[derive(Clone, Debug)]
pub enum RestResPacifica<T> {
    Error(PacificaError),
    Data(Vec<T>),
    Object(T),
}

#[derive(Clone, Debug, Deserialize)]
pub struct PacificaError {
    #[serde(default)]
    pub code: Option<i64>,
    #[serde(default)]
    pub error: Option<String>,
}

impl<T> IntoInfraData<T> for RestResPacifica<T> {
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
            Self::Error(PacificaError { code, error }) => {
                let code = code.map_or_else(|| "none".to_string(), |code| code.to_string());
                let error = error.unwrap_or_default();
                warn!("Pacifica REST error {}: {}", code, error);
                Err(InfraError::ApiCliError(format!(
                    "Pacifica REST error (code={}): {}",
                    code, error
                )))
            },
        }
    }
}

impl<'de, T> Deserialize<'de> for RestResPacifica<T>
where
    T: DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut value = Value::deserialize(deserializer)?;

        match value.get("success").and_then(Value::as_bool) {
            Some(true) => {},
            Some(false) => {
                return serde_json::from_value(value)
                    .map(Self::Error)
                    .map_err(D::Error::custom);
            },
            None => return Err(D::Error::missing_field("success")),
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

    #[derive(Debug, Deserialize)]
    struct Book {
        s: String,
    }

    #[test]
    fn object_and_list_payloads_parse() {
        let one: RestResPacifica<Book> = serde_json::from_str(
            r#"{"success":true,"data":{"s":"NVDA","l":[[],[]],"t":1790586168392},"error":null,"code":null}"#,
        )
        .unwrap();
        let list: RestResPacifica<Book> = serde_json::from_str(
            r#"{"success":true,"data":[{"s":"BTC"},{"s":"SOL-USDC"}],"error":null,"code":null}"#,
        )
        .unwrap();

        assert_eq!(one.into_one().unwrap().s, "NVDA");
        assert_eq!(list.into_vec().unwrap().len(), 2);
    }

    #[test]
    fn failure_is_an_error_with_the_venue_code_and_message() {
        let res: RestResPacifica<Book> = serde_json::from_str(
            r#"{"success":false,"data":null,"error":"Book not found: NOPE","code":400,"error_id":"unspecified"}"#,
        )
        .unwrap();

        assert!(matches!(
            res,
            RestResPacifica::Error(PacificaError {
                code: Some(400),
                ..
            })
        ));
        let err = res.into_vec().unwrap_err().to_string();
        assert!(
            err.contains("code=400") && err.contains("Book not found: NOPE"),
            "{err}"
        );
    }

    #[test]
    fn failure_without_code_is_still_an_error() {
        let res: RestResPacifica<Book> =
            serde_json::from_str(r#"{"success":false,"data":null,"error":"Not found"}"#).unwrap();

        assert!(
            res.into_one()
                .unwrap_err()
                .to_string()
                .contains("Not found")
        );
    }

    #[test]
    fn into_one_rejects_lists_that_are_not_single() {
        let empty: RestResPacifica<Book> =
            serde_json::from_str(r#"{"success":true,"data":[]}"#).unwrap();
        let several: RestResPacifica<Book> =
            serde_json::from_str(r#"{"success":true,"data":[{"s":"A"},{"s":"B"}]}"#).unwrap();

        assert!(empty.into_one().is_err());
        assert!(several.into_one().is_err());
    }

    #[test]
    fn malformed_payloads_are_parse_errors() {
        assert!(serde_json::from_str::<RestResPacifica<Book>>(r#"{"success":true}"#).is_err());
        assert!(serde_json::from_str::<RestResPacifica<Book>>(r#"{"s":"BTC"}"#).is_err());
        assert!(
            serde_json::from_str::<RestResPacifica<Book>>(r#"{"success":true,"data":{"x":1}}"#)
                .is_err()
        );
    }
}
