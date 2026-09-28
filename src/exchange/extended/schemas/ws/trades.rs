use serde::Deserialize;

use extrema_infra::{
    arch::market_assets::api_general::ts_to_micros,
    prelude::{IntoWsData, OrderSide, WsTrade},
};

use crate::exchange::extended::{api_utils::extended_market_to_cli, config_assets::EXTENDED};

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsTradeExtended {
    i: u64,               // Trade ID
    m: String,            // Market
    S: ExtendedTakerSide, // Taker side
    T: u64,               // Trade time
    p: String,            // Price
    q: String,            // Quantity
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
enum ExtendedTakerSide {
    Buy,
    Sell,
}

impl IntoWsData for WsTradeExtended {
    type Output = WsTrade;

    fn into_ws(self) -> WsTrade {
        WsTrade {
            timestamp: ts_to_micros(self.T),
            market: EXTENDED,
            inst: extended_market_to_cli(&self.m),
            price: self.p.parse().unwrap_or_default(),
            size: self.q.parse().unwrap_or_default(),
            side: match self.S {
                ExtendedTakerSide::Buy => OrderSide::BUY,
                ExtendedTakerSide::Sell => OrderSide::SELL,
            },
            trade_id: self.i,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::extended::extended_ws_msg::ExtendedWsData;

    use super::*;

    const FRAME: &str = r#"{"data":[{"i":2104498792354025474,"m":"BTC-USD","S":"SELL","tT":"TRADE",
        "T":1790586586451,"p":"82575","q":"0.00020"},{"i":2104498792354025475,"m":"BTC-USD","S":"SELL",
        "tT":"TRADE","T":1790586586451,"p":"82574","q":"0.00106"}],"ts":1790586586457,"seq":6}"#;

    fn decode(frame: &str) -> serde_json::Result<Vec<WsTrade>> {
        ExtendedWsData::<WsTradeExtended>::decode_trades(frame.as_bytes()).map(|d| d.into_ws())
    }

    #[test]
    fn live_frame_becomes_trades() {
        let trades = decode(FRAME).unwrap();

        assert_eq!(trades.len(), 2);
        assert_eq!(trades[0].market, EXTENDED);
        assert_eq!(trades[0].inst, "BTC_USD_PERP");
        assert_eq!(trades[0].side, OrderSide::SELL);
        assert_eq!((trades[0].price, trades[0].size), (82575.0, 0.0002));
        assert_eq!(trades[0].trade_id, 2104498792354025474);
        assert_eq!(trades[0].timestamp, 1_790_586_586_451_000);
        assert_eq!(trades[1].trade_id, 2104498792354025475);
    }

    #[test]
    fn liquidations_and_deleverages_are_trades_too() {
        let frame = FRAME
            .replacen(r#""tT":"TRADE""#, r#""tT":"LIQUIDATION""#, 1)
            .replacen(r#""S":"SELL""#, r#""S":"BUY""#, 1);
        let trades = decode(&frame).unwrap();

        assert_eq!(trades.len(), 2);
        assert_eq!(trades[0].side, OrderSide::BUY);
    }

    #[test]
    fn history_frame_is_not_emitted() {
        let history = FRAME.replace(r#""seq":6"#, r#""seq":1"#);

        assert!(decode(&history).unwrap().is_empty());
    }

    #[test]
    fn missing_side_or_non_numeric_id_fails_the_frame() {
        let no_side = FRAME.replacen(r#""S":"SELL","#, "", 1);
        let text_id = FRAME.replacen("2104498792354025474", r#""2104498792354025474""#, 1);
        let bad_side = FRAME.replacen(r#""S":"SELL""#, r#""S":"ASK""#, 1);

        for frame in [no_side, text_id, bad_side] {
            assert!(decode(&frame).is_err(), "{frame}");
        }
    }
}
