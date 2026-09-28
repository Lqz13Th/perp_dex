use serde::{Deserialize, Deserializer, de::DeserializeOwned, de::Error as DeError};
use serde_json::Value;
use tracing::warn;

use extrema_infra::prelude::{InfraError, InfraResult, IntoInfraData};

use crate::exchange::api_general::exactly_one;

/// Arcus replies with the payload itself, or `{"error": "..."}` on failure.
#[derive(Clone, Debug)]
pub enum RestResArcus<T> {
    Error(ArcusError),
    Object(T),
}

#[derive(Clone, Debug, Deserialize)]
pub struct ArcusError {
    pub error: String,
}

impl<T> IntoInfraData<T> for RestResArcus<T> {
    fn into_one(self) -> InfraResult<T> {
        match self {
            Self::Object(o) => Ok(o),
            other => exactly_one(other.into_vec()?),
        }
    }

    fn into_vec(self) -> InfraResult<Vec<T>> {
        match self {
            Self::Object(o) => Ok(vec![o]),
            Self::Error(ArcusError { error }) => {
                warn!("Arcus REST error: {}", error);
                Err(InfraError::ApiCliError(format!(
                    "Arcus REST error: {}",
                    error
                )))
            },
        }
    }
}

impl<'de, T> Deserialize<'de> for RestResArcus<T>
where
    T: DeserializeOwned,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;

        if value.get("error").is_some_and(Value::is_string) {
            return serde_json::from_value(value)
                .map(Self::Error)
                .map_err(D::Error::custom);
        }

        serde_json::from_value(value)
            .map(Self::Object)
            .map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize)]
    struct Payload {
        markets: Vec<u8>,
    }

    #[test]
    fn payload_parses_as_object() {
        let res: RestResArcus<Payload> = serde_json::from_str(r#"{"markets":[1,2]}"#).unwrap();

        assert_eq!(res.into_one().unwrap().markets, vec![1, 2]);
    }

    #[test]
    fn error_payload_is_an_error() {
        let res: RestResArcus<Payload> =
            serde_json::from_str(r#"{"error":"Unknown market: NOPE-USD"}"#).unwrap();

        let err = res.into_vec().unwrap_err().to_string();
        assert!(err.contains("Unknown market: NOPE-USD"), "{err}");
    }

    #[test]
    fn malformed_payload_is_a_parse_error() {
        assert!(serde_json::from_str::<RestResArcus<Payload>>(r#"{"other":1}"#).is_err());
    }
}
