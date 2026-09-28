use serde::Deserialize;

use extrema_infra::{
    arch::market_assets::api_general::ts_to_micros,
    prelude::{IntoWsData, OrderSide, WsTrade},
};

use crate::exchange::pacifica::{api_utils::pacifica_symbol_to_cli, config_assets::PACIFICA};

/// Taker side of each fill; `h` is the fill's history id.
#[derive(Clone, Debug, Deserialize)]
pub(crate) struct WsTradePacifica {
    h: u64,
    s: String,
    a: String,
    p: String,
    d: PacificaTakerSide,
    t: u64,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PacificaTakerSide {
    OpenLong,
    CloseShort,
    OpenShort,
    CloseLong,
}

impl IntoWsData for WsTradePacifica {
    type Output = WsTrade;

    fn into_ws(self) -> WsTrade {
        WsTrade {
            timestamp: ts_to_micros(self.t),
            market: PACIFICA,
            inst: pacifica_symbol_to_cli(&self.s),
            price: self.p.parse().unwrap_or_default(),
            size: self.a.parse().unwrap_or_default(),
            side: match self.d {
                PacificaTakerSide::OpenLong | PacificaTakerSide::CloseShort => OrderSide::BUY,
                PacificaTakerSide::OpenShort | PacificaTakerSide::CloseLong => OrderSide::SELL,
            },
            trade_id: self.h,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::exchange::pacifica::pacifica_ws_msg::PacificaWsData;

    use super::*;

    const FRAME: &str = r#"{"channel":"trades","data":[
        {"h":287350861,"s":"SOL","a":"0.01","p":"118.47","d":"close_long","tc":"normal","t":1790586037495,"li":13379867567,"it":0},
        {"h":287350863,"s":"SOL","a":"4.22","p":"118.48","d":"close_short","tc":"normal","t":1790586037498,"li":13379867568,"it":0}]}"#;

    fn decode(frame: &str) -> serde_json::Result<Vec<WsTrade>> {
        PacificaWsData::<WsTradePacifica>::decode_batch(frame.as_bytes()).map(|d| d.into_ws())
    }

    #[test]
    fn batched_fills_become_trades() {
        let trades = decode(FRAME).unwrap();

        assert_eq!(trades.len(), 2);
        assert_eq!(trades[0].market, PACIFICA);
        assert_eq!(trades[0].inst, "SOL_USDC_PERP");
        assert_eq!(trades[0].price, 118.47);
        assert_eq!(trades[0].size, 0.01);
        assert_eq!(trades[0].side, OrderSide::SELL);
        assert_eq!(trades[0].trade_id, 287350861);
        assert_eq!(trades[0].timestamp, 1_790_586_037_495_000);
        assert_eq!(trades[1].side, OrderSide::BUY);
        assert_eq!(trades[1].trade_id, 287350863);
    }

    #[test]
    fn opening_sides_follow_the_taker() {
        let open_long = FRAME.replacen("close_long", "open_long", 1);
        let open_short = FRAME.replacen("close_short", "open_short", 1);

        assert_eq!(decode(&open_long).unwrap()[0].side, OrderSide::BUY);
        assert_eq!(decode(&open_short).unwrap()[1].side, OrderSide::SELL);
    }

    #[test]
    fn liquidation_fills_are_trades() {
        let frame = FRAME.replacen(r#""tc":"normal""#, r#""tc":"market_liquidation""#, 1);

        assert_eq!(decode(&frame).unwrap().len(), 2);
    }

    #[test]
    fn unknown_side_or_non_numeric_id_fails_the_frame() {
        let side = FRAME.replacen(r#""d":"close_long""#, r#""d":"buy""#, 1);
        let id = FRAME.replacen(r#""h":287350861"#, r#""h":"trade-1""#, 1);
        let no_id = FRAME.replacen(r#""h":287350861,"#, "", 1);

        for frame in [side, id, no_id] {
            assert!(decode(&frame).is_err(), "{frame}");
        }
    }
}
