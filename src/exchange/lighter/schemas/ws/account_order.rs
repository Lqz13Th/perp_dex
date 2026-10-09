use std::collections::BTreeMap;

use serde::Deserialize;

use extrema_infra::prelude::{IntoWsData, Market, WsAccOrder};

use crate::exchange::lighter::{
    api_utils::lighter_market_to_cli, schemas::rest::open_order::OpenOrderLighter,
};

/// `account_all_orders/{account}`: the orders one transaction changed, keyed by market; the subscribe reply
/// lists the open orders.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsAccountOrdersLighter<const ID: u16> {
    pub orders: BTreeMap<String, Vec<OpenOrderLighter>>,
}

impl<const ID: u16> IntoWsData for WsAccountOrdersLighter<ID> {
    type Output = Vec<WsAccOrder>;

    fn into_ws(self) -> Vec<WsAccOrder> {
        self.orders
            .into_values()
            .flatten()
            .map(|o| WsAccOrder {
                timestamp: o.update_time(),
                market: Market::Custom(ID),
                inst: lighter_market_to_cli(o.market_index),
                inst_type: o.inst_type(),
                price: o.price.parse().unwrap_or_default(),
                size: o.initial_base_amount.parse().unwrap_or_default(),
                filled_size: o.executed_size(),
                side: o.side(),
                status: o.status(),
                order_type: o.kind(),
                order_id: Some(o.order_index.to_string()),
                cli_order_id: o.cli_order_id(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use extrema_infra::prelude::{InstrumentType, OrderSide, OrderStatus, OrderType};

    use crate::exchange::lighter::{
        config_assets::{LIGHTER, LIGHTER_MARKET_ID},
        lighter_ws_msg::LighterWsAccountData,
    };

    use super::*;

    const OPEN: &[u8] = br#"{"channel":"account_all_orders:758666","orders":{"139":[{"order_index":39687971468375991,
        "client_order_index":528503094,"order_id":"39687971468375991","client_order_id":"528503094","market_index":139,
        "market_kind":"perps","owner_account_index":758666,"initial_base_amount":"0.0071","price":"1562.15",
        "nonce":281474728884151,"remaining_base_amount":"0.0071","is_ask":false,"base_size":71,"base_price":156215,
        "filled_base_amount":"0.0000","filled_quote_amount":"0.000000","side":"","type":"limit",
        "time_in_force":"post-only","reduce_only":false,"trigger_price":"0.00","order_expiry":1793947703202,
        "status":"open","trigger_status":"na","trigger_time":0,"parent_order_index":0,"parent_order_id":"0",
        "to_trigger_order_id_0":"0","to_trigger_order_id_1":"0","to_cancel_order_id_0":"0",
        "integrator_fee_collector_index":"0","integrator_taker_fee":"0","integrator_maker_fee":"0","order_flags":0,
        "block_height":352005024,"timestamp":1791528502,"created_at":1791528502,"updated_at":1791528502,
        "transaction_time":1791528503344183,"order_version":0}]},"type":"update/account_all_orders"}"#;

    fn decode(frame: &[u8]) -> Vec<WsAccOrder> {
        LighterWsAccountData::<WsAccountOrdersLighter<LIGHTER_MARKET_ID>>::decode(frame)
            .unwrap()
            .into_ws()
    }

    #[test]
    fn order_updates_become_account_orders() {
        let orders = decode(OPEN);
        assert_eq!(orders.len(), 1);
        let o = &orders[0];
        assert_eq!(o.market, LIGHTER);
        assert_eq!(o.inst, "@139");
        assert_eq!(o.inst_type, InstrumentType::Perpetual);
        assert_eq!(o.side, OrderSide::BUY);
        assert_eq!(o.order_type, OrderType::PostOnly);
        assert_eq!(o.status, OrderStatus::Live);
        assert_eq!(o.order_id.as_deref(), Some("39687971468375991"));
        assert_eq!(o.cli_order_id.as_deref(), Some("528503094"));
        assert!((o.price - 1562.15).abs() < 1e-9 && (o.size - 0.0071).abs() < 1e-12);
        assert_eq!((o.filled_size, o.timestamp), (0.0, 1791528503344183));
    }

    #[test]
    fn partial_fills_cancels_and_snapshots_map() {
        let partial = String::from_utf8(OPEN.to_vec()).unwrap().replace(
            r#""filled_base_amount":"0.0000""#,
            r#""filled_base_amount":"0.0030""#,
        );
        let o = &decode(partial.as_bytes())[0];
        assert_eq!(o.status, OrderStatus::PartiallyFilled);
        assert_eq!(o.filled_size, 0.003);

        let canceled = String::from_utf8(OPEN.to_vec())
            .unwrap()
            .replace(r#""status":"open""#, r#""status":"canceled-post-only""#);
        assert_eq!(decode(canceled.as_bytes())[0].status, OrderStatus::Canceled);

        let snapshot = br#"{"channel":"account_all_orders:758666","orders":{},"type":"subscribed/account_all_orders"}"#;
        assert!(decode(snapshot).is_empty());
    }
}
