use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::price_data::OrderBookData, api_general::get_micros_timestamp,
};

/// Resting orders, best price first; `into_orderbook_data` sums them into price levels.
#[derive(Clone, Debug, Deserialize)]
pub struct RestOrderBookOrdersLighter {
    pub asks: Vec<RestBookOrderLighter>,
    pub bids: Vec<RestBookOrderLighter>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RestBookOrderLighter {
    pub price: String,
    pub remaining_base_amount: String,
}

impl RestOrderBookOrdersLighter {
    pub fn into_orderbook_data(self, inst: &str, depth: usize) -> OrderBookData {
        OrderBookData {
            timestamp: get_micros_timestamp(),
            inst: inst.to_string(),
            bids: orders_to_levels(self.bids, depth),
            asks: orders_to_levels(self.asks, depth),
        }
    }
}

fn orders_to_levels(orders: Vec<RestBookOrderLighter>, depth: usize) -> Vec<(f64, f64)> {
    let mut levels: Vec<(String, f64)> = Vec::new();

    for order in orders {
        let size: f64 = order.remaining_base_amount.parse().unwrap_or_default();
        match levels.last_mut() {
            Some((price, total)) if *price == order.price => *total += size,
            _ => {
                if depth > 0 && levels.len() == depth {
                    break;
                }
                levels.push((order.price, size));
            },
        }
    }

    levels
        .into_iter()
        .map(|(price, size)| (price.parse().unwrap_or_default(), size))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = r#"{"code":200,"total_asks":3,"asks":[
        {"order_index":1,"price":"223.405","remaining_base_amount":"45.600","initial_base_amount":"45.600"},
        {"order_index":2,"price":"223.405","remaining_base_amount":"4.400","initial_base_amount":"5"},
        {"order_index":3,"price":"223.420","remaining_base_amount":"1.000","initial_base_amount":"1"}],
        "total_bids":2,"bids":[
        {"order_index":4,"price":"223.390","remaining_base_amount":"2.000","initial_base_amount":"2"},
        {"order_index":5,"price":"223.380","remaining_base_amount":"3.500","initial_base_amount":"4"}]}"#;

    #[test]
    fn orders_at_one_price_sum_into_a_level() {
        let book = serde_json::from_str::<RestOrderBookOrdersLighter>(RAW)
            .unwrap()
            .into_orderbook_data("@110", 0);

        assert_eq!(book.inst, "@110");
        assert_eq!(book.asks, vec![(223.405, 50.0), (223.42, 1.0)]);
        assert_eq!(book.bids, vec![(223.39, 2.0), (223.38, 3.5)]);
    }

    #[test]
    fn depth_caps_levels_not_orders() {
        let book = serde_json::from_str::<RestOrderBookOrdersLighter>(RAW)
            .unwrap()
            .into_orderbook_data("@110", 1);

        assert_eq!(book.asks, vec![(223.405, 50.0)]);
        assert_eq!(book.bids, vec![(223.39, 2.0)]);
    }

    #[test]
    fn empty_book_is_empty() {
        let book = serde_json::from_str::<RestOrderBookOrdersLighter>(
            r#"{"code":200,"total_asks":0,"asks":[],"total_bids":0,"bids":[]}"#,
        )
        .unwrap()
        .into_orderbook_data("@9999", 5);

        assert!(book.asks.is_empty() && book.bids.is_empty());
    }
}
