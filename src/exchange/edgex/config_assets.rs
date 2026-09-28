use extrema_infra::prelude::Market;

pub const EDGEX_MARKET_ID: u16 = 7;
pub const EDGEX: Market = Market::Custom(EDGEX_MARKET_ID);

pub const EDGEX_BASE_URL: &str = "https://edgex-prod-v2.edgex.exchange";
pub const EDGEX_META_DATA: &str = "/api/v2/public/meta/getMetaData";
pub const EDGEX_TICKER: &str = "/api/v2/public/quote/getTicker";
pub const EDGEX_DEPTH: &str = "/api/v2/public/quote/getDepth";

pub const EDGEX_WS: &str = "wss://edgex-quote-prod-v2.edgex.exchange/api/v1/public/ws";
/// Every market's BBO once a second; the per-market `bookTicker.{id}` only sends its snapshot.
pub const EDGEX_WS_BBO_ALL: &str = "bookTicker.all.1s";

/// The only book depths edgeX serves, over REST and websocket alike.
pub const EDGEX_BOOK_LEVELS: [u16; 2] = [15, 200];
pub const EDGEX_MAX_BOOK_LEVELS: u16 = 200;

/// Tickers are one contract per request; this many run at once.
pub const EDGEX_TICKER_CONCURRENCY: usize = 16;
