use serde::{Deserialize, Deserializer, de::Error as DeError};

use extrema_infra::{
    arch::market_assets::api_general::ts_to_micros,
    prelude::{IntoWsData, OrderSide, WsTrade},
};

use crate::exchange::apex::{api_utils::apex_symbol_to_cli, config_assets::APEX};

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsTradeApex {
    T: u64,           // Trade time
    s: String,        // Symbol
    S: ApexTakerSide, // Taker side
    v: String,        // Size
    p: String,        // Price
    #[serde(deserialize_with = "de_uuid_to_u64")]
    i: u64, // Trade id
}

#[derive(Clone, Copy, Debug, Deserialize)]
enum ApexTakerSide {
    Buy,
    Sell,
}

/// ApeX trade ids are UUIDs; the 128 bits are folded to 64 (`high ^ low`), so the id is stable but not reversible.
fn de_uuid_to_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let uuid = String::deserialize(deserializer)?;
    let groups: Vec<&str> = uuid.split('-').collect();
    let is_uuid = groups.iter().map(|g| g.len()).eq([8, 4, 4, 4, 12])
        && groups
            .iter()
            .all(|g| g.bytes().all(|b| b.is_ascii_hexdigit()));

    if !is_uuid {
        return Err(D::Error::custom(format!(
            "ApeX trade id is not a UUID: {uuid}"
        )));
    }

    let id = u128::from_str_radix(&groups.concat(), 16).map_err(D::Error::custom)?;
    Ok((id >> 64) as u64 ^ id as u64)
}

impl IntoWsData for WsTradeApex {
    type Output = WsTrade;

    fn into_ws(self) -> WsTrade {
        WsTrade {
            timestamp: ts_to_micros(self.T),
            market: APEX,
            inst: apex_symbol_to_cli(&self.s),
            price: self.p.parse().unwrap_or_default(),
            size: self.v.parse().unwrap_or_default(),
            side: match self.S {
                ApexTakerSide::Buy => OrderSide::BUY,
                ApexTakerSide::Sell => OrderSide::SELL,
            },
            trade_id: self.i,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::apex::apex_ws_msg::ApexWsData;

    use super::*;

    const FRAME: &str = r#"{"topic":"recentlyTrade.H.BTCUSDT","type":"delta","data":[
        {"T":1790586423481,"s":"BTCUSDT","S":"Sell","v":"0.002","p":"82700.0","L":"MinusTick","i":"504b0e2f-341b-5e8a-a7aa-047de3051df5"},
        {"T":1790586423481,"s":"BTCUSDT","S":"Sell","v":"0.006","p":"82696.8","L":"MinusTick","i":"d129dda6-c5fd-525e-8ce3-693858ab4c8f"},
        {"T":1790586423481,"s":"BTCUSDT","S":"Sell","v":"0.004","p":"82688.2","L":"MinusTick","i":"c20dd37f-dac7-5e5d-a0f8-342658e75d38"}],
        "cs":66026503066,"ts":1790586423571534}"#;

    #[test]
    fn one_sweep_becomes_trades_in_fill_order() {
        let trades = ApexWsData::<WsTradeApex>::decode_batch(FRAME.as_bytes())
            .unwrap()
            .into_ws();

        assert_eq!(trades.len(), 3);
        assert_eq!(trades[0].market, APEX);
        assert_eq!(trades[0].inst, "BTC_USDT_PERP");
        assert_eq!(trades[0].side, OrderSide::SELL);
        assert_eq!((trades[0].price, trades[0].size), (82700.0, 0.002));
        assert_eq!(trades[2].price, 82688.2);
        assert_eq!(trades[0].timestamp, 1_790_586_423_481_000);
        assert_eq!(trades[0].trade_id, 0x504b0e2f341b5e8a ^ 0xa7aa047de3051df5);
        assert_ne!(trades[0].trade_id, trades[1].trade_id);
    }

    #[test]
    fn buy_is_the_taker_side() {
        let frame =
            br#"{"topic":"recentlyTrade.H.BTCUSDT","type":"delta","data":[{"T":1790586438720,
            "s":"BTCUSDT","S":"Buy","v":"0.211","p":"82754.3","L":"PlusTick",
            "i":"ec7c09e0-6394-51bb-bd6e-ef9249499034"}],"cs":66026512160,"ts":1790586438771597}"#;

        let trade = ApexWsData::<WsTradeApex>::decode_batch(frame)
            .unwrap()
            .into_ws()
            .pop()
            .unwrap();

        assert_eq!(trade.side, OrderSide::BUY);
    }

    #[test]
    fn missing_side_or_malformed_id_fails_the_frame() {
        let no_side = FRAME.replacen(r#""S":"Sell","#, "", 1);
        let bad_side = FRAME.replacen(r#""S":"Sell""#, r#""S":"sell""#, 1);
        let numeric_id = FRAME.replacen(
            r#""i":"504b0e2f-341b-5e8a-a7aa-047de3051df5""#,
            r#""i":"7808871""#,
            1,
        );
        let bad_hex = FRAME.replacen("504b0e2f-", "504b0e2g-", 1);

        for frame in [no_side, bad_side, numeric_id, bad_hex] {
            assert!(
                ApexWsData::<WsTradeApex>::decode_batch(frame.as_bytes()).is_err(),
                "{frame}"
            );
        }
    }

    #[test]
    fn trade_decoder_rejects_book_frames() {
        let book = br#"{"topic":"orderBook25.H.BTCUSDT","type":"delta","data":{"s":"BTCUSDT",
            "b":[],"a":[["82909.5","0"]],"u":5234003},"cs":66025718860,"ts":1790585479521674}"#;

        assert!(ApexWsData::<WsTradeApex>::decode_batch(book).is_err());
    }
}
