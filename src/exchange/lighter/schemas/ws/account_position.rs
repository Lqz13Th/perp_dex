use std::collections::BTreeMap;

use serde::Deserialize;

use extrema_infra::prelude::{InstrumentType, IntoWsData, MarginMode, PositionSide, WsAccPosition};

use crate::exchange::lighter::{
    api_utils::lighter_market_to_cli, schemas::rest::account::AccountPositionLighter,
};

/// `account_all_positions/{account}`: positions keyed by market, flat ones included.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsAccountPositionsLighter<const ID: u16> {
    pub positions: BTreeMap<String, AccountPositionLighter>,
}

impl<const ID: u16> IntoWsData for WsAccountPositionsLighter<ID> {
    type Output = Vec<WsAccPosition>;

    fn into_ws(self) -> Vec<WsAccPosition> {
        self.positions
            .into_values()
            .map(|p| {
                let size = p.size();
                WsAccPosition {
                    inst: lighter_market_to_cli(p.market_id),
                    inst_type: InstrumentType::Perpetual,
                    avg_price: p.avg_entry_price.parse().unwrap_or_default(),
                    size,
                    position_side: if size > 0.0 {
                        PositionSide::Long
                    } else if size < 0.0 {
                        PositionSide::Short
                    } else {
                        PositionSide::Both
                    },
                    margin_mode: match p.margin_mode {
                        0 => MarginMode::Cross,
                        1 => MarginMode::Isolated,
                        _ => MarginMode::Unknown,
                    },
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::lighter::{
        config_assets::LIGHTER_MARKET_ID, lighter_ws_msg::LighterWsAccountData,
    };

    use super::*;

    #[test]
    fn positions_map_with_sign_and_margin_mode() {
        let frame = br#"{"bo_positions":{},"channel":"account_all_positions:758666","positions":{
            "139":{"market_id":139,"symbol":"SNDK","initial_margin_fraction":"20.00","open_order_count":1,
              "pending_order_count":0,"position_tied_order_count":0,"sign":-1,"position":"0.0071",
              "avg_entry_price":"1644.40","position_value":"-11.675","unrealized_pnl":"0.000000",
              "realized_pnl":"0.000000","liquidation_price":"1900","margin_mode":1,"margin_set_flag":1,
              "allocated_margin":"2.400000"},
            "110":{"market_id":110,"symbol":"NVDA","initial_margin_fraction":"10.00","sign":1,"position":"0.0000",
              "avg_entry_price":"0.00","position_value":"0.000000","margin_mode":0,"allocated_margin":"0.000000"}},
            "shares":[],"transaction_time":1791528503344183,"type":"update/account_all_positions"}"#;
        let p = LighterWsAccountData::<WsAccountPositionsLighter<LIGHTER_MARKET_ID>>::decode(frame)
            .unwrap()
            .into_ws();
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].inst.as_str(), p[0].size), ("@110", 0.0));
        assert_eq!(p[0].position_side, PositionSide::Both);
        assert_eq!(p[0].margin_mode, MarginMode::Cross);
        assert_eq!((p[1].inst.as_str(), p[1].size), ("@139", -0.0071));
        assert_eq!(p[1].position_side, PositionSide::Short);
        assert_eq!(p[1].margin_mode, MarginMode::Isolated);
        assert!((p[1].avg_price - 1644.4).abs() < 1e-9);
    }
}
