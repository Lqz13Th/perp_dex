use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::account_data::OrderDetailData,
    base_data::{OrderSide, OrderStatus, OrderType, TimeInForce},
};

use crate::exchange::lighter::api_utils::lighter_market_to_cli;

/// `GET /api/v1/accountActiveOrders` (auth token in the `authorization` header).
#[derive(Clone, Debug, Deserialize)]
pub struct RestOpenOrdersLighter {
    #[serde(default)]
    pub orders: Vec<OpenOrderLighter>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OpenOrderLighter {
    pub order_index: i64,
    pub client_order_index: i64,
    pub market_index: u16,
    pub initial_base_amount: String,
    pub remaining_base_amount: String,
    pub filled_base_amount: String,
    #[serde(default)]
    pub filled_quote_amount: String,
    pub price: String,
    pub is_ask: bool,
    #[serde(rename = "type")]
    pub order_type: String,
    pub time_in_force: String,
    pub reduce_only: bool,
    pub status: String,
    #[serde(default)]
    pub timestamp: u64,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub transaction_time: u64,
}

fn num(s: &str) -> f64 {
    s.parse().unwrap_or_default()
}

/// Lighter order times are seconds (`timestamp`, `updated_at`) except `transaction_time` (microseconds).
fn to_micros(t: u64) -> u64 {
    match t {
        0..1_000_000_000_000 => t * 1_000_000,
        1_000_000_000_000..1_000_000_000_000_000 => t * 1_000,
        _ => t,
    }
}

pub fn lighter_order_status(status: &str, filled: f64) -> OrderStatus {
    match status {
        "open" | "pending" | "in-progress" if filled > 0.0 => OrderStatus::PartiallyFilled,
        "open" | "pending" | "in-progress" => OrderStatus::Live,
        "filled" => OrderStatus::Filled,
        "canceled-expired" => OrderStatus::Expired,
        s if s.starts_with("canceled") => OrderStatus::Canceled,
        _ => OrderStatus::Unknown,
    }
}

impl OpenOrderLighter {
    pub fn into_order_detail_data(self) -> OrderDetailData {
        let executed = num(&self.filled_base_amount);
        let avg = if executed > 0.0 {
            num(&self.filled_quote_amount) / executed
        } else {
            0.0
        };
        OrderDetailData {
            timestamp: to_micros(self.timestamp),
            inst: lighter_market_to_cli(self.market_index),
            order_id: self.order_index.to_string(),
            cli_order_id: (self.client_order_index != 0)
                .then(|| self.client_order_index.to_string()),
            side: if self.is_ask {
                OrderSide::SELL
            } else {
                OrderSide::BUY
            },
            position_side: None,
            order_type: match (self.order_type.as_str(), self.time_in_force.as_str()) {
                ("market", _) => OrderType::Market,
                (_, "post-only") => OrderType::PostOnly,
                (_, "immediate-or-cancel") => OrderType::Ioc,
                ("limit", _) => OrderType::Limit,
                _ => OrderType::Unknown,
            },
            order_status: lighter_order_status(&self.status, executed),
            price: num(&self.price),
            avg_price: avg,
            size: num(&self.initial_base_amount),
            executed_size: executed,
            fee: None,
            fee_currency: None,
            reduce_only: Some(self.reduce_only),
            time_in_force: Some(match self.time_in_force.as_str() {
                "immediate-or-cancel" => TimeInForce::IOC,
                "good-till-time" | "post-only" => TimeInForce::GTD,
                _ => TimeInForce::Unknown,
            }),
            update_time: to_micros(
                self.transaction_time
                    .max(self.updated_at)
                    .max(self.timestamp),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resting_post_only_maps_to_order_detail() {
        let raw = r#"{"code":200,"orders":[{"order_index":39687971468506323,"client_order_index":1791518274,
            "order_id":"39687971468506323","client_order_id":"1791518274","market_index":139,
            "owner_account_index":758666,"initial_base_amount":"0.0072","price":"1552.96","nonce":5,
            "remaining_base_amount":"0.0072","is_ask":false,"base_size":72,"base_price":155296,
            "filled_base_amount":"0.0000","filled_quote_amount":"0.000000","side":"","type":"limit",
            "time_in_force":"post-only","reduce_only":false,"trigger_price":"0.00","order_expiry":1794110274000,
            "status":"open","trigger_status":"na","trigger_time":0,"block_height":1,"timestamp":1791519491,
            "created_at":1791519491,"updated_at":1791519491,"transaction_time":1791519491569218}]}"#;
        let o: RestOpenOrdersLighter = serde_json::from_str(raw).unwrap();
        let d = o
            .orders
            .into_iter()
            .next()
            .unwrap()
            .into_order_detail_data();
        assert_eq!(
            (d.inst.as_str(), d.order_id.as_str()),
            ("@139", "39687971468506323")
        );
        assert_eq!(d.cli_order_id.as_deref(), Some("1791518274"));
        assert_eq!(
            (d.side, d.order_type, d.order_status),
            (OrderSide::BUY, OrderType::PostOnly, OrderStatus::Live)
        );
        assert!((d.price - 1552.96).abs() < 1e-9 && (d.size - 0.0072).abs() < 1e-12);
        assert_eq!(
            (d.timestamp, d.update_time),
            (1791519491000000, 1791519491569218)
        );
    }

    #[test]
    fn statuses() {
        assert_eq!(
            lighter_order_status("open", 0.1),
            OrderStatus::PartiallyFilled
        );
        assert_eq!(
            lighter_order_status("canceled-post-only", 0.0),
            OrderStatus::Canceled
        );
        assert_eq!(
            lighter_order_status("canceled-expired", 0.0),
            OrderStatus::Expired
        );
        assert_eq!(lighter_order_status("filled", 1.0), OrderStatus::Filled);
    }
}
