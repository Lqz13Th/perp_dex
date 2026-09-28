use std::time::Duration;

use extrema_infra::prelude::Market;

pub const LIGHTER_MARKET_ID: u16 = 1;
pub const LIGHTER: Market = Market::Custom(LIGHTER_MARKET_ID);
/// Lighter on Robinhood Chain: same protocol, separate books, accounts and market ids.
pub const LIGHTER_RH_MARKET_ID: u16 = 4;
pub const LIGHTER_RH: Market = Market::Custom(LIGHTER_RH_MARKET_ID);

pub const LIGHTER_BASE_URL: &str = "https://mainnet.zklighter.elliot.ai";
pub const LIGHTER_RH_BASE_URL: &str = "https://api.rh.lighter.xyz";
pub const LIGHTER_ORDER_BOOK_DETAILS: &str = "/api/v1/orderBookDetails";
pub const LIGHTER_ORDER_BOOK_ORDERS: &str = "/api/v1/orderBookOrders";

pub const LIGHTER_WS: &str = "wss://mainnet.zklighter.elliot.ai/stream";
pub const LIGHTER_RH_WS: &str = "wss://api.rh.lighter.xyz/stream";
/// Lighter drops a connection after two minutes without a client frame, however busy the stream is.
pub const LIGHTER_WS_PING: &str = r#"{"type":"ping"}"#;
pub const LIGHTER_WS_PING_INTERVAL: Duration = Duration::from_secs(30);

pub const LIGHTER_ORDER_BOOK_ORDERS_LIMIT: usize = 250;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LighterVenue {
    #[default]
    Mainnet,
    Robinhood,
}

impl LighterVenue {
    pub fn market(self) -> Market {
        match self {
            LighterVenue::Mainnet => LIGHTER,
            LighterVenue::Robinhood => LIGHTER_RH,
        }
    }

    pub fn base_url(self) -> &'static str {
        match self {
            LighterVenue::Mainnet => LIGHTER_BASE_URL,
            LighterVenue::Robinhood => LIGHTER_RH_BASE_URL,
        }
    }

    pub fn ws_url(self) -> &'static str {
        match self {
            LighterVenue::Mainnet => LIGHTER_WS,
            LighterVenue::Robinhood => LIGHTER_RH_WS,
        }
    }
}
