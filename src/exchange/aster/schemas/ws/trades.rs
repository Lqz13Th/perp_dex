use serde::Deserialize;

use extrema_infra::{
    arch::market_assets::api_general::ts_to_micros,
    prelude::{IntoWsData, OrderSide, WsTrade},
};

use crate::exchange::aster::{api_utils::aster_inst_to_cli, config_assets::ASTER};

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsAggTradeAster {
    a: u64,    // Aggregate trade ID
    s: String, // Symbol
    p: String, // Price
    q: String, // Quantity
    T: u64,    // Trade time
    m: bool,   // Is the buyer the market maker?
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsTradeAster {
    t: u64,    // Trade ID
    s: String, // Symbol
    p: String, // Price
    q: String, // Quantity
    T: u64,    // Trade time
    m: bool,   // Is the buyer the market maker?
}

fn aster_taker_side(buyer_is_maker: bool) -> OrderSide {
    if buyer_is_maker {
        OrderSide::SELL
    } else {
        OrderSide::BUY
    }
}

impl IntoWsData for WsAggTradeAster {
    type Output = WsTrade;

    fn into_ws(self) -> WsTrade {
        WsTrade {
            timestamp: ts_to_micros(self.T),
            market: ASTER,
            inst: aster_inst_to_cli(&self.s),
            price: self.p.parse().unwrap_or_default(),
            size: self.q.parse().unwrap_or_default(),
            side: aster_taker_side(self.m),
            trade_id: self.a,
        }
    }
}

impl IntoWsData for WsTradeAster {
    type Output = WsTrade;

    fn into_ws(self) -> WsTrade {
        WsTrade {
            timestamp: ts_to_micros(self.T),
            market: ASTER,
            inst: aster_inst_to_cli(&self.s),
            price: self.p.parse().unwrap_or_default(),
            size: self.q.parse().unwrap_or_default(),
            side: aster_taker_side(self.m),
            trade_id: self.t,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::aster::aster_ws_msg::AsterWsData;

    use super::*;

    #[test]
    fn decodes_individual_trade() {
        let frame = br#"{"e":"trade","E":1790577908458,"T":1790577908400,"s":"BTCUSDT",
            "t":148203357,"p":"83146.4","q":"0.001","X":"MARKET","m":true}"#;

        let trade = AsterWsData::<WsTradeAster>::decode_single(frame)
            .unwrap()
            .into_ws()
            .pop()
            .unwrap();

        assert_eq!(trade.market, ASTER);
        assert_eq!(trade.inst, "BTC_USDT_PERP");
        assert_eq!(trade.price, 83_146.4);
        assert_eq!(trade.size, 0.001);
        assert_eq!(trade.side, OrderSide::SELL);
        assert_eq!(trade.trade_id, 148203357);
        assert_eq!(trade.timestamp, 1_790_577_908_400_000);
    }

    #[test]
    fn decodes_aggregate_trade() {
        let frame = br#"{"e":"aggTrade","E":1790577843114,"a":987654321,"s":"NVDAUSDT",
            "p":"223.40","q":"1.25","f":100,"l":101,"T":1790577843113,"m":false}"#;

        let trade = AsterWsData::<WsAggTradeAster>::decode_single(frame)
            .unwrap()
            .into_ws()
            .pop()
            .unwrap();

        assert_eq!(trade.inst, "NVDA_USDT_PERP");
        assert_eq!(trade.side, OrderSide::BUY);
        assert_eq!(trade.trade_id, 987654321);
    }

    #[test]
    fn trade_decoder_rejects_aggregate_frames() {
        let frame = br#"{"e":"aggTrade","E":1,"a":9,"s":"NVDAUSDT","p":"1","q":"1","f":1,"l":1,"T":1,"m":false}"#;

        assert!(AsterWsData::<WsTradeAster>::decode_single(frame).is_err());
    }
}
