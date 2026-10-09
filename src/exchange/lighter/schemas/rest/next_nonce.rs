use serde::Deserialize;

/// `GET /api/v1/nextNonce`.
#[derive(Clone, Debug, Deserialize)]
pub struct RestNextNonceLighter {
    pub nonce: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::lighter::lighter_rest_msg::RestResLighter;
    use extrema_infra::prelude::IntoInfraData;

    #[test]
    fn next_nonce_parses() {
        let n: RestResLighter<RestNextNonceLighter> =
            serde_json::from_str(r#"{"code":200,"nonce":5}"#).unwrap();
        assert_eq!(n.into_one().unwrap().nonce, 5);
    }
}
