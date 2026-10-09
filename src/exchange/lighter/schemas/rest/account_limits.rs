use serde::Deserialize;

/// `GET /api/v1/accountLimits`: the account tier (`standard`: no fees, delayed orders; `premium`: fees, no
/// delay) and the fee ticks it trades at.
#[derive(Clone, Debug, Deserialize)]
pub struct RestAccountLimitsLighter {
    pub user_tier: String,
    #[serde(default)]
    pub current_maker_fee_tick: i64,
    #[serde(default)]
    pub current_taker_fee_tick: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::lighter::lighter_rest_msg::RestResLighter;
    use extrema_infra::prelude::IntoInfraData;

    #[test]
    fn standard_account_limits_parse() {
        let r: RestResLighter<RestAccountLimitsLighter> = serde_json::from_str(
            r#"{"code":200,"max_llp_percentage":100,"max_llp_amount":"0.000000","user_tier":"standard",
            "can_create_public_pool":false,"user_tier_name":"standard","current_maker_fee_tick":0,
            "current_taker_fee_tick":0,"leased_lit":"0.00000000","effective_lit_stakes":"0.00000000",
            "user_tier_last_update":0}"#,
        )
        .unwrap();
        let l = r.into_one().unwrap();
        assert_eq!(l.user_tier, "standard");
        assert_eq!((l.current_maker_fee_tick, l.current_taker_fee_tick), (0, 0));
    }
}
