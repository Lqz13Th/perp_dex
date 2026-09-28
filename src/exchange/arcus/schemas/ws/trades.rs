use serde::Deserialize;

use extrema_infra::{
    arch::market_assets::api_general::{de_u64_from_string_or_number, ts_to_micros},
    prelude::{IntoWsData, OrderSide, WsTrade},
};

use crate::exchange::arcus::{api_utils::arcus_market_to_cli, config_assets::ARCUS};

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsTradeArcus {
    marketDisplayName: String,
    side: ArcusTakerSide,
    price: String,
    size: String,
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    tradeId: u64,
    timestamp: u64,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
enum ArcusTakerSide {
    Buy,
    Sell,
}

impl IntoWsData for WsTradeArcus {
    type Output = WsTrade;

    fn into_ws(self) -> WsTrade {
        WsTrade {
            timestamp: ts_to_micros(self.timestamp),
            market: ARCUS,
            inst: arcus_market_to_cli(&self.marketDisplayName),
            price: self.price.parse().unwrap_or_default(),
            size: self.size.parse().unwrap_or_default(),
            side: match self.side {
                ArcusTakerSide::Buy => OrderSide::BUY,
                ArcusTakerSide::Sell => OrderSide::SELL,
            },
            trade_id: self.tradeId,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::arcus::arcus_ws_msg::ArcusWsData;

    use super::*;

    const FRAME: &str = r#"{"type":"channel_data","channel":"trades","id":"BTC-USD","contents":[
        {"marketDisplayName":"BTC-USD","marketId":1,"side":"SELL","price":"83136","size":"0.005",
         "tradeId":"7808871","timestamp":1790577907374882,"makerOrderId":"b4ecb1e514c44b6f",
         "takerOrderId":"c107f2f1e4d44a9e","sequenceNumber":2615227423},
        {"marketDisplayName":"BTC-USD","marketId":1,"side":"SELL","price":"83135.7","size":"0.00631498",
         "tradeId":"7808872","timestamp":1790577907374882,"takerOrderId":"c107f2f1e4d44a9e"}]}"#;

    #[test]
    fn one_taker_order_fills_become_trades() {
        let trades = ArcusWsData::<WsTradeArcus>::decode_trades(FRAME.as_bytes())
            .unwrap()
            .into_ws();

        assert_eq!(trades.len(), 2);
        assert_eq!(trades[0].market, ARCUS);
        assert_eq!(trades[0].inst, "BTC_USD_PERP");
        assert_eq!(trades[0].side, OrderSide::SELL);
        assert_eq!(trades[0].price, 83136.0);
        assert_eq!(trades[0].trade_id, 7808871);
        assert_eq!(trades[0].timestamp, 1_790_577_907_374_882);
        assert_eq!(trades[1].trade_id, 7808872);
    }

    #[test]
    fn missing_side_or_non_numeric_id_fails_the_frame() {
        let no_side = FRAME.replacen(r#""side":"SELL","#, "", 1);
        let text_id = FRAME.replacen(r#""tradeId":"7808871""#, r#""tradeId":"trade-789""#, 1);

        assert!(ArcusWsData::<WsTradeArcus>::decode_trades(no_side.as_bytes()).is_err());
        assert!(ArcusWsData::<WsTradeArcus>::decode_trades(text_id.as_bytes()).is_err());
    }
}
