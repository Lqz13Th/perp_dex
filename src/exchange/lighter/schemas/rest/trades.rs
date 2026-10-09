use serde::Deserialize;

use extrema_infra::prelude::OrderSide;

use crate::exchange::lighter::{
    api_utils::lighter_market_to_cli, schemas::rest::open_order::to_micros,
};

/// `GET /api/v1/trades?account_index=…` (auth token in the `authorization` header).
#[derive(Clone, Debug, Deserialize)]
pub struct RestTradesLighter {
    #[serde(default)]
    pub trades: Vec<TradeLighter>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

/// One print; the account channels push the same objects.
#[derive(Clone, Debug, Deserialize)]
pub struct TradeLighter {
    pub trade_id: u64,
    #[serde(default)]
    pub tx_hash: String,
    /// `trade`, `liquidation`, `deleverage` or `market-settlement`
    #[serde(rename = "type", default)]
    pub kind: String,
    pub market_id: u16,
    pub size: String,
    pub price: String,
    pub ask_id: i64,
    pub bid_id: i64,
    #[serde(default)]
    pub ask_client_id: i64,
    #[serde(default)]
    pub bid_client_id: i64,
    pub ask_account_id: i64,
    pub bid_account_id: i64,
    pub is_maker_ask: bool,
    #[serde(default)]
    pub timestamp: u64,
    #[serde(default)]
    pub transaction_time: u64,
    #[serde(default)]
    pub taker_fee: i64,
    #[serde(default)]
    pub maker_fee: i64,
}

/// One fill of the account.
#[derive(Clone, Debug, PartialEq)]
pub struct LighterFill {
    pub timestamp: u64,
    pub inst: String,
    pub trade_id: u64,
    pub order_id: String,
    pub cli_order_id: Option<String>,
    pub side: OrderSide,
    pub price: f64,
    pub size: f64,
    pub is_maker: bool,
    /// The trade's `maker_fee` / `taker_fee` as sent (absent, so 0, without fees).
    pub fee_raw: i64,
    /// `trade`, `liquidation`, `deleverage` or `market-settlement`
    pub kind: String,
    pub tx_hash: String,
}

impl TradeLighter {
    /// The side of the trade that belongs to `account_index`, if any.
    pub fn fill_of(&self, account_index: i64) -> Option<LighterFill> {
        let (side, order_id, cli_order_id, is_maker) = if self.bid_account_id == account_index {
            (
                OrderSide::BUY,
                self.bid_id,
                self.bid_client_id,
                !self.is_maker_ask,
            )
        } else if self.ask_account_id == account_index {
            (
                OrderSide::SELL,
                self.ask_id,
                self.ask_client_id,
                self.is_maker_ask,
            )
        } else {
            return None;
        };
        Some(LighterFill {
            timestamp: to_micros(self.transaction_time.max(self.timestamp)),
            inst: lighter_market_to_cli(self.market_id),
            trade_id: self.trade_id,
            order_id: order_id.to_string(),
            cli_order_id: (cli_order_id != 0).then(|| cli_order_id.to_string()),
            side,
            price: self.price.parse().unwrap_or_default(),
            size: self.size.parse().unwrap_or_default(),
            is_maker,
            fee_raw: if is_maker {
                self.maker_fee
            } else {
                self.taker_fee
            },
            kind: self.kind.clone(),
            tx_hash: self.tx_hash.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRADE: &str = r#"{"trade_id":33911049803,"trade_id_str":"33911049803",
        "tx_hash":"5c86db34","type":"trade","market_id":139,"market_kind":"perps","size":"0.1520",
        "price":"1644.40","usd_amount":"249.948800","ask_id":39406496975952292,"ask_id_str":"39406496975952292",
        "bid_id":39687971468374688,"bid_id_str":"39687971468374688","ask_client_id":22719913,
        "ask_client_id_str":"22719913","bid_client_id":527249189,"bid_client_id_str":"527249189",
        "ask_account_id":281474976696055,"bid_account_id":281474976493954,"is_maker_ask":true,
        "block_height":352005692,"timestamp":1791528611944,"taker_position_size_before":"10.2565",
        "taker_entry_quote_before":"16838.010771","taker_initial_margin_fraction_before":1000,"maker_fee":32,
        "maker_position_size_before":"-9.6057","maker_entry_quote_before":"15792.089766",
        "maker_initial_margin_fraction_before":1000,"transaction_time":1791528612051882,
        "ask_order_version":0,"bid_order_version":0}"#;

    #[test]
    fn each_side_of_a_trade_becomes_that_account_fill() {
        let t: TradeLighter = serde_json::from_str(TRADE).unwrap();

        let maker = t.fill_of(281474976696055).unwrap();
        assert_eq!(maker.side, OrderSide::SELL);
        assert_eq!((maker.is_maker, maker.fee_raw), (true, 32));
        assert_eq!(
            (maker.order_id.as_str(), maker.cli_order_id.as_deref()),
            ("39406496975952292", Some("22719913"))
        );
        assert_eq!(
            (maker.inst.as_str(), maker.timestamp),
            ("@139", 1791528612051882)
        );
        assert!((maker.price - 1644.4).abs() < 1e-9 && (maker.size - 0.152).abs() < 1e-12);

        let taker = t.fill_of(281474976493954).unwrap();
        assert_eq!(taker.side, OrderSide::BUY);
        assert_eq!((taker.is_maker, taker.fee_raw), (false, 0));
        assert_eq!(taker.order_id, "39687971468374688");

        assert!(t.fill_of(758666).is_none());
    }

    #[test]
    fn trade_pages_parse() {
        let page: RestTradesLighter = serde_json::from_str(&format!(
            r#"{{"code":200,"next_cursor":"eyJp","trades":[{TRADE}]}}"#
        ))
        .unwrap();
        assert_eq!(page.trades.len(), 1);
        assert_eq!(page.next_cursor.as_deref(), Some("eyJp"));
        let empty: RestTradesLighter = serde_json::from_str(r#"{"code":200,"trades":[]}"#).unwrap();
        assert!(empty.trades.is_empty() && empty.next_cursor.is_none());
    }
}
