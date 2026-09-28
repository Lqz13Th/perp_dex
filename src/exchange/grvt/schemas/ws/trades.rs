use serde::{Deserialize, Deserializer, de::Error as DeError};

use extrema_infra::{
    arch::market_assets::api_general::de_u64_from_string_or_number,
    prelude::{IntoWsData, OrderSide, WsTrade},
};

use crate::exchange::grvt::{
    api_utils::{grvt_inst_to_cli, grvt_ns_to_micros, grvt_trade_id_to_u64},
    config_assets::GRVT,
};

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsTradeGrvt {
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    event_time: u64,
    instrument: String,
    is_taker_buyer: bool,
    size: String,
    price: String,
    #[serde(deserialize_with = "de_grvt_trade_id")]
    trade_id: u64,
}

fn de_grvt_trade_id<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let trade_id = String::deserialize(deserializer)?;

    grvt_trade_id_to_u64(&trade_id)
        .ok_or_else(|| D::Error::custom(format!("invalid GRVT trade_id: {trade_id}")))
}

impl IntoWsData for WsTradeGrvt {
    type Output = WsTrade;

    fn into_ws(self) -> WsTrade {
        WsTrade {
            timestamp: grvt_ns_to_micros(self.event_time),
            market: GRVT,
            inst: grvt_inst_to_cli(&self.instrument),
            price: self.price.parse().unwrap_or_default(),
            size: self.size.parse().unwrap_or_default(),
            side: if self.is_taker_buyer {
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
    use crate::exchange::grvt::grvt_ws_msg::GrvtWsData;

    use super::*;

    const FRAME: &str = r#"{"stream":"v1.trade","selector":"ETH_USDT_Perp@50","sequence_number":"9599",
        "feed":{"event_time":"1790586069683993871","instrument":"ETH_USDT_Perp","is_taker_buyer":true,
        "size":"0.334","price":"2651.87","mark_price":"2651.543208812","index_price":"2652.740465649",
        "interest_rate":"0.0","forward_price":"0.0","trade_id":"198827910-1","venue":"ORDERBOOK",
        "is_rpi":false},"prev_sequence_number":"9598"}"#;

    fn decode(frame: &str) -> serde_json::Result<Vec<WsTrade>> {
        GrvtWsData::<WsTradeGrvt>::decode_trades(frame.as_bytes()).map(|data| data.into_ws())
    }

    #[test]
    fn live_fill_is_a_trade() {
        let trades = decode(FRAME).unwrap();

        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].market, GRVT);
        assert_eq!(trades[0].inst, "ETH_USDT_PERP");
        assert_eq!(trades[0].price, 2651.87);
        assert_eq!(trades[0].size, 0.334);
        assert_eq!(trades[0].side, OrderSide::BUY);
        assert_eq!(trades[0].trade_id, 198_827_910_000_001);
        assert_eq!(trades[0].timestamp, 1_790_586_069_683_993);
    }

    #[test]
    fn taker_sell_is_a_sell() {
        let frame = FRAME.replace(r#""is_taker_buyer":true"#, r#""is_taker_buyer":false"#);

        assert_eq!(decode(&frame).unwrap()[0].side, OrderSide::SELL);
    }

    #[test]
    fn replayed_fill_is_not_a_trade() {
        let frame = FRAME
            .replace(r#""sequence_number":"9599""#, r#""sequence_number":"0""#)
            .replace(
                r#""prev_sequence_number":"9598""#,
                r#""prev_sequence_number":"0""#,
            );

        assert!(decode(&frame).unwrap().is_empty());
    }

    #[test]
    fn missing_side_or_bad_trade_id_fails_the_frame() {
        let no_side = FRAME.replacen(r#""is_taker_buyer":true,"#, "", 1);
        let plain_id = FRAME.replacen(r#""198827910-1""#, r#""198827910""#, 1);
        let text_id = FRAME.replacen(r#""198827910-1""#, r#""trade-789""#, 1);

        assert!(decode(&no_side).is_err());
        assert!(decode(&plain_id).is_err());
        assert!(decode(&text_id).is_err());
    }
}
