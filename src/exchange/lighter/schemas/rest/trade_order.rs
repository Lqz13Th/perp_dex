use serde::Deserialize;

/// `POST /api/v1/sendTx`.
#[derive(Clone, Debug, Deserialize)]
pub struct RestSendTxLighter {
    pub tx_hash: String,
    #[serde(default)]
    pub predicted_execution_time_ms: i64,
    #[serde(default)]
    pub volume_quota_remaining: i64,
}

/// `POST /api/v1/sendTxBatch`: one hash per transaction, in order.
#[derive(Clone, Debug, Deserialize)]
pub struct RestSendTxBatchLighter {
    #[serde(default)]
    pub tx_hash: Vec<String>,
    #[serde(default)]
    pub predicted_execution_time_ms: i64,
    #[serde(default)]
    pub volume_quota_remaining: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::lighter::lighter_rest_msg::RestResLighter;
    use extrema_infra::prelude::IntoInfraData;

    #[test]
    fn send_replies_parse_and_errors_are_errors() {
        let r: RestResLighter<RestSendTxLighter> = serde_json::from_str(
            r#"{"code":200,"message":"{\"ratelimit\": \"didn't use volume quota\"}","tx_hash":"8fc80084","predicted_execution_time_ms":1791518275043,"volume_quota_remaining":999}"#,
        )
        .unwrap();
        assert_eq!(r.into_one().unwrap().tx_hash, "8fc80084");
        let b: RestResLighter<RestSendTxBatchLighter> =
            serde_json::from_str(r#"{"code":200,"tx_hash":["a","b"]}"#).unwrap();
        assert_eq!(b.into_one().unwrap().tx_hash, vec!["a", "b"]);
        let e: RestResLighter<RestSendTxLighter> =
            serde_json::from_str(r#"{"code":21104,"message":"invalid nonce"}"#).unwrap();
        assert!(e.into_one().is_err());
    }
}
