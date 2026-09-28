use serde::Deserialize;

use extrema_infra::{
    arch::market_assets::api_general::de_u64_from_string_or_number,
    prelude::{IntoWsData, OrderSide, WsTrade},
};

use crate::exchange::nado::{
    api_utils::{nado_ns_to_micros, nado_product_to_cli, nado_x18_to_f64},
    config_assets::NADO,
};

/// One match per frame. Nado sends no trade id, so `trade_id` is 0; the matches
/// of one taker order share a `timestamp`.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsTradeNado {
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    timestamp: u64,
    product_id: u32,
    price: String,
    taker_qty: String,
    is_taker_buyer: bool,
}

impl IntoWsData for WsTradeNado {
    type Output = WsTrade;

    fn into_ws(self) -> WsTrade {
        WsTrade {
            timestamp: nado_ns_to_micros(self.timestamp),
            market: NADO,
            inst: nado_product_to_cli(self.product_id),
            price: nado_x18_to_f64(&self.price),
            size: nado_x18_to_f64(&self.taker_qty),
            side: if self.is_taker_buyer {
                OrderSide::BUY
            } else {
                OrderSide::SELL
            },
            trade_id: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::nado::nado_ws_msg::NadoWsData;

    use super::*;

    const SELL: &str = r#"{"type":"trade","timestamp":"1790586002468925586","product_id":2,
        "price":"82920000000000000000000","taker_qty":"3850000000000000","maker_qty":"3850000000000000",
        "is_taker_buyer":false}"#;

    fn decode(frame: &str) -> serde_json::Result<Vec<WsTrade>> {
        NadoWsData::<WsTradeNado>::decode_single(frame.as_bytes()).map(|data| data.into_ws())
    }

    #[test]
    fn match_becomes_a_trade_on_the_aggressor_side() {
        let trade = decode(SELL).unwrap().pop().unwrap();

        assert_eq!(trade.market, NADO);
        assert_eq!(trade.inst, "@2");
        assert_eq!(trade.price, 82920.0);
        assert_eq!(trade.size, 0.00385);
        assert_eq!(trade.side, OrderSide::SELL);
        assert_eq!(trade.trade_id, 0);
        assert_eq!(trade.timestamp, 1_790_586_002_468_925);

        let buy = decode(r#"{"type":"trade","timestamp":"1790586018672806347","product_id":112,
            "price":"223840000000000000000","taker_qty":"810000000000000000","maker_qty":"810000000000000000",
            "is_taker_buyer":true}"#)
        .unwrap()
        .pop()
        .unwrap();
        assert_eq!(
            (buy.inst.as_str(), buy.price, buy.size, buy.side),
            ("@112", 223.84, 0.81, OrderSide::BUY)
        );
    }

    #[test]
    fn missing_side_or_bad_timestamp_fails_the_frame() {
        let no_side = SELL.replacen(r#""is_taker_buyer":false"#, r#""is_maker_buyer":true"#, 1);
        let text_time = SELL.replacen(r#""1790586002468925586""#, r#""soon""#, 1);

        assert!(decode(&no_side).is_err());
        assert!(decode(&text_time).is_err());
    }

    #[test]
    fn subscribe_ack_is_not_a_trade() {
        assert!(decode(r#"{"result":null,"id":1}"#).unwrap().is_empty());
    }
}
