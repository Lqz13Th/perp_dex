use serde::{Deserialize, Deserializer, de::Error as DeError};

use extrema_infra::{
    arch::market_assets::api_general::{de_u64_from_string_or_number, ts_to_micros},
    prelude::{IntoWsData, OrderSide, WsTrade},
};

use crate::exchange::edgex::{api_utils::edgex_contract_to_cli, config_assets::EDGEX};

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsTradeEdgex {
    #[serde(deserialize_with = "de_edgex_trade_id")]
    ticketId: u64,
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    time: u64,
    price: String,
    size: String,
    #[serde(deserialize_with = "de_u64_from_string_or_number")]
    contractId: u64,
    isBuyerMaker: bool,
}

/// edgeX identifies a fill only by its UUID `ticketId`; the trade id is its low 64 bits.
fn edgex_trade_id(ticket_id: &str) -> Option<u64> {
    let canonical = ticket_id.len() == 36
        && ticket_id.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
    if !canonical {
        return None;
    }

    u64::from_str_radix(&ticket_id[19..].replace('-', ""), 16).ok()
}

fn de_edgex_trade_id<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let ticket_id = String::deserialize(deserializer)?;

    edgex_trade_id(&ticket_id)
        .ok_or_else(|| D::Error::custom(format!("edgeX ticketId is not a UUID: {ticket_id}")))
}

impl IntoWsData for WsTradeEdgex {
    type Output = WsTrade;

    fn into_ws(self) -> WsTrade {
        WsTrade {
            timestamp: ts_to_micros(self.time),
            market: EDGEX,
            inst: edgex_contract_to_cli(self.contractId),
            price: self.price.parse().unwrap_or_default(),
            size: self.size.parse().unwrap_or_default(),
            side: if self.isBuyerMaker {
                OrderSide::SELL
            } else {
                OrderSide::BUY
            },
            trade_id: self.ticketId,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::edgex::edgex_ws_msg::EdgexWsData;

    use super::*;

    const FRAME: &str = r#"{"type":"quote-event","channel":"trades.30000001","content":{
        "channel":"trades.30000001","dataType":"changed","data":[
        {"ticketId":"f3d5df60-f4f8-41c6-adea-f11af6cfb459","time":"1790585548197","price":"82851.2",
         "size":"0.104","value":"8616.5248","takerOrderId":"799373727132287382",
         "makerOrderId":"799373720496900897","takerAccountId":"100000000000000101",
         "makerAccountId":"100000000000000102","contractId":"30000001","contractName":"BTCUSDC",
         "isBestMatch":false,"isBuyerMaker":true},
        {"ticketId":"939b878c-55b9-4746-8136-fc9bd076b4c5","time":"1790585548238","price":"82851.3",
         "size":"0.004","value":"331.4052","takerOrderId":"799373727312642454",
         "makerOrderId":"799373727308448150","takerAccountId":"100000000000000103",
         "makerAccountId":"100000000000000104","contractId":"30000001","contractName":"BTCUSDC",
         "isBestMatch":true,"isBuyerMaker":false}]}}"#;

    #[test]
    fn live_fills_become_trades_with_the_aggressor_side() {
        let trades = EdgexWsData::<WsTradeEdgex>::decode_trades(FRAME.as_bytes())
            .unwrap()
            .into_ws();

        assert_eq!(trades.len(), 2);
        assert_eq!(trades[0].market, EDGEX);
        assert_eq!(trades[0].inst, "@30000001");
        assert_eq!(trades[0].price, 82_851.2);
        assert_eq!(trades[0].size, 0.104);
        assert_eq!(trades[0].side, OrderSide::SELL);
        assert_eq!(trades[0].timestamp, 1_790_585_548_197_000);
        assert_eq!(trades[0].trade_id, 0xadea_f11a_f6cf_b459);
        assert_eq!(trades[1].side, OrderSide::BUY);
        assert_eq!(trades[1].trade_id, 0x8136_fc9b_d076_b4c5);
    }

    #[test]
    fn trade_id_is_the_low_half_of_the_uuid() {
        assert_eq!(
            edgex_trade_id("aba7e32e-af00-4067-ab1d-f35257550f2e"),
            Some(0xab1d_f352_5755_0f2e)
        );
        assert_eq!(
            edgex_trade_id("ABA7E32E-AF00-4067-AB1D-F35257550F2E"),
            Some(0xab1d_f352_5755_0f2e)
        );
        for bad in [
            "",
            "987654321",
            "aba7e32eaf004067ab1df35257550f2e",
            "aba7e32e-af00-4067-ab1d-f35257550f2",
            "aba7e32e-af00-4067-ab1d-f35257550f2g",
            "aba7e32e-af00-4067+ab1d-f35257550f2e",
        ] {
            assert_eq!(edgex_trade_id(bad), None, "{bad}");
        }
    }

    #[test]
    fn missing_side_or_non_uuid_id_fails_the_frame() {
        let no_side = FRAME.replacen(r#""isBuyerMaker":true"#, r#""isBuyerMaker":null"#, 1);
        let numeric_id = FRAME.replacen(
            r#""ticketId":"f3d5df60-f4f8-41c6-adea-f11af6cfb459""#,
            r#""ticketId":987654321"#,
            1,
        );
        let text_id = FRAME.replacen(
            r#""ticketId":"f3d5df60-f4f8-41c6-adea-f11af6cfb459""#,
            r#""ticketId":"trade-789""#,
            1,
        );

        for frame in [no_side, numeric_id, text_id] {
            assert!(EdgexWsData::<WsTradeEdgex>::decode_trades(frame.as_bytes()).is_err());
        }
    }
}
