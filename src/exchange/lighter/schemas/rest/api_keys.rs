use serde::Deserialize;

/// `GET /api/v1/apikeys`.
#[derive(Clone, Debug, Deserialize)]
pub struct RestApiKeysLighter {
    #[serde(default)]
    pub api_keys: Vec<ApiKeyLighter>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ApiKeyLighter {
    pub account_index: i64,
    pub api_key_index: u8,
    pub nonce: i64,
    pub public_key: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::lighter::lighter_rest_msg::RestResLighter;
    use extrema_infra::prelude::IntoInfraData;

    #[test]
    fn api_keys_parse() {
        let k: RestResLighter<RestApiKeysLighter> = serde_json::from_str(
            r#"{"code":200,"api_keys":[{"account_index":758666,"api_key_index":4,"nonce":5,"public_key":"6f5a","transaction_time":1}]}"#,
        )
        .unwrap();
        assert_eq!(k.into_one().unwrap().api_keys[0].public_key, "6f5a");
    }
}
