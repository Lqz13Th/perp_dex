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
/// `WsChannel::Other` of a private connection that only sends transactions (`jsonapi/sendtx`), no subscription.
pub const LIGHTER_TX_CHANNEL: &str = "lighter_send_tx";
pub const LIGHTER_WS_ACCOUNT_ORDERS: &str = "account_all_orders";
pub const LIGHTER_WS_ACCOUNT_POSITIONS: &str = "account_all_positions";
pub const LIGHTER_WS_ACCOUNT_TRADES: &str = "account_all_trades";

pub const LIGHTER_ORDER_BOOK_ORDERS_LIMIT: usize = 250;

pub const LIGHTER_ACCOUNT: &str = "/api/v1/account";
pub const LIGHTER_ACCOUNT_ACTIVE_ORDERS: &str = "/api/v1/accountActiveOrders";
pub const LIGHTER_ACCOUNT_INACTIVE_ORDERS: &str = "/api/v1/accountInactiveOrders";
pub const LIGHTER_ACCOUNT_ORDERS: &str = "/api/v1/accountOrders";
pub const LIGHTER_TRADES: &str = "/api/v1/trades";
pub const LIGHTER_ACCOUNT_LIMITS: &str = "/api/v1/accountLimits";
pub const LIGHTER_POSITION_FUNDING: &str = "/api/v1/positionFunding";
pub const LIGHTER_API_KEYS: &str = "/api/v1/apikeys";
pub const LIGHTER_NEXT_NONCE: &str = "/api/v1/nextNonce";
pub const LIGHTER_SEND_TX: &str = "/api/v1/sendTx";
pub const LIGHTER_SEND_TX_BATCH: &str = "/api/v1/sendTxBatch";

pub const LIGHTER_CHAIN_ID: u32 = 304;
pub const LIGHTER_RH_CHAIN_ID: u32 = 466324;
/// Transactions are valid for this long after signing (lighter-go's default, 10 min minus a second).
pub const LIGHTER_TX_TTL_MS: i64 = 599_000;
/// Expiry of resting (GTT / post-only) orders.
pub const LIGHTER_ORDER_TTL_MS: i64 = 28 * 24 * 3_600_000;
/// Checked when a request is made or a channel subscribed; a subscription outlives its token.
pub const LIGHTER_AUTH_TOKEN_TTL_S: u64 = 600;
pub const LIGHTER_SEND_TX_BATCH_MAX: usize = 15;
pub const LIGHTER_HISTORY_PAGE_MAX: u32 = 100;
pub const LIGHTER_ACCOUNT_ORDERS_MAX: usize = 20;

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

    pub fn chain_id(self) -> u32 {
        match self {
            LighterVenue::Mainnet => LIGHTER_CHAIN_ID,
            LighterVenue::Robinhood => LIGHTER_RH_CHAIN_ID,
        }
    }

    pub fn ws_url(self) -> &'static str {
        match self {
            LighterVenue::Mainnet => LIGHTER_WS,
            LighterVenue::Robinhood => LIGHTER_RH_WS,
        }
    }
}
