use std::collections::BTreeMap;

use serde::Deserialize;

use crate::exchange::lighter::schemas::rest::trades::{LighterFill, TradeLighter};

/// `account_all_trades/{account}` on a `WsChannel::Other` task: parse `WsOtherMessage::raw_json` with serde.
#[derive(Clone, Debug, Deserialize)]
pub struct WsAccountTradesLighter {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub trades: BTreeMap<String, Vec<TradeLighter>>,
}

impl WsAccountTradesLighter {
    /// Fills of `account_index` in a live update; the subscribe reply's history is skipped.
    pub fn into_fills(self, account_index: i64) -> Vec<LighterFill> {
        if !self.kind.starts_with("update/") {
            return Vec::new();
        }
        self.trades
            .into_values()
            .flatten()
            .filter_map(|t| t.fill_of(account_index))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use extrema_infra::prelude::OrderSide;

    use super::*;

    const TRADE: &str = r#"{"trade_id":7,"tx_hash":"ab","type":"trade","market_id":139,"size":"0.0071",
        "price":"1644.40","usd_amount":"11.675","ask_id":1,"bid_id":39687971468375991,"ask_client_id":0,
        "bid_client_id":528503094,"ask_account_id":1234,"bid_account_id":758666,"is_maker_ask":true,
        "block_height":1,"timestamp":1791528611944,"transaction_time":1791528612051882}"#;

    #[test]
    fn live_updates_become_fills_of_the_account() {
        let frame = format!(
            r#"{{"channel":"account_all_trades:758666","trades":{{"139":[{TRADE}]}},"type":"update/account_all_trades"}}"#
        );
        let fills = serde_json::from_str::<WsAccountTradesLighter>(&frame)
            .unwrap()
            .into_fills(758666);
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].side, OrderSide::BUY);
        assert_eq!(
            (fills[0].is_maker, fills[0].cli_order_id.as_deref()),
            (false, Some("528503094"))
        );
    }

    #[test]
    fn subscribe_history_is_skipped() {
        let frame = format!(
            r#"{{"channel":"account_all_trades:758666","daily_volume":0,"trades":{{"139":[{TRADE}]}},"type":"subscribed/account_all_trades"}}"#
        );
        let t: WsAccountTradesLighter = serde_json::from_str(&frame).unwrap();
        assert!(t.into_fills(758666).is_empty());
    }
}
