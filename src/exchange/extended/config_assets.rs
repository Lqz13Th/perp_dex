use extrema_infra::prelude::Market;

pub const EXTENDED_MARKET_ID: u16 = 6;
pub const EXTENDED: Market = Market::Custom(EXTENDED_MARKET_ID);

pub const EXTENDED_BASE_URL: &str = "https://api.starknet.extended.exchange";
pub const EXTENDED_MARKETS: &str = "/api/v1/info/markets";
pub const EXTENDED_ORDER_BOOK: &str = "/orderbook";
/// The REST API answers 403 to requests without a `User-Agent`.
pub const EXTENDED_USER_AGENT: &str =
    concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

pub const EXTENDED_WS: &str = "wss://api.starknet.extended.exchange/stream.extended.exchange/v1";
