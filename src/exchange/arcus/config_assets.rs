use extrema_infra::prelude::Market;

pub const ARCUS_MARKET_ID: u16 = 3;
pub const ARCUS: Market = Market::Custom(ARCUS_MARKET_ID);

pub const ARCUS_BASE_URL: &str = "https://api.arcus.xyz";
pub const ARCUS_MARKETS: &str = "/v1/markets";
pub const ARCUS_L2_ORDER_BOOK: &str = "/v1/l2OrderBook";

pub const ARCUS_WS: &str = "wss://api.arcus.xyz/v1/ws";

pub const ARCUS_MAX_BOOK_LEVELS: usize = 100;
