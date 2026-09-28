use serde::Deserialize;

use extrema_infra::{
    arch::market_assets::api_general::ts_to_micros,
    prelude::{IntoWsData, Market, OrderSide, WsTrade},
};

use crate::exchange::lighter::api_utils::lighter_market_to_cli;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsTradeLighter<const ID: u16> {
    trade_id: u64,
    market_id: u16,
    price: String,
    size: String,
    is_maker_ask: bool,
    timestamp: u64,
}

impl<const ID: u16> IntoWsData for WsTradeLighter<ID> {
    type Output = WsTrade;

    fn into_ws(self) -> WsTrade {
        WsTrade {
            timestamp: ts_to_micros(self.timestamp),
            market: Market::Custom(ID),
            inst: lighter_market_to_cli(self.market_id),
            price: self.price.parse().unwrap_or_default(),
            size: self.size.parse().unwrap_or_default(),
            side: if self.is_maker_ask {
                OrderSide::BUY
            } else {
                OrderSide::SELL
            },
            trade_id: self.trade_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::lighter::{
        config_assets::{LIGHTER, LIGHTER_MARKET_ID},
        lighter_ws_msg::LighterWsData,
    };

    use super::*;

    const FRAME: &[u8] = br#"{"channel":"trade:1","liquidation_trades":[
        {"trade_id":32311807101,"type":"liquidation","market_id":1,"size":"0.5","price":"83100.0",
         "is_maker_ask":false,"timestamp":1790577908476}],
        "nonce":23189370832,"trades":[
        {"trade_id":32311807100,"trade_id_str":"32311807100","type":"trade","market_id":1,
         "market_kind":"perps","size":"0.00026","price":"83131.9","usd_amount":"21.614294",
         "ask_id":562953436031865,"bid_id":844421410564150,"is_maker_ask":true,
         "block_height":343313872,"timestamp":1790577908475,"taker_fee":50,"maker_fee":28}],
        "type":"update/trade"}"#;

    #[test]
    fn live_trades_and_liquidations_become_trades() {
        let trades = LighterWsData::<WsTradeLighter<LIGHTER_MARKET_ID>>::decode_trades(FRAME)
            .unwrap()
            .into_ws();

        assert_eq!(trades.len(), 2);
        assert_eq!(trades[0].market, LIGHTER);
        assert_eq!(trades[0].inst, "@1");
        assert_eq!(trades[0].price, 83_131.9);
        assert_eq!(trades[0].size, 0.00026);
        assert_eq!(trades[0].side, OrderSide::BUY);
        assert_eq!(trades[0].trade_id, 32311807100);
        assert_eq!(trades[0].timestamp, 1_790_577_908_475_000);
        assert_eq!(trades[1].side, OrderSide::SELL);
        assert_eq!(trades[1].trade_id, 32311807101);
    }
}
