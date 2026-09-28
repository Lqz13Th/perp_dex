use std::time::Duration;

use extrema_infra::prelude::Market;

pub const APEX_MARKET_ID: u16 = 8;
pub const APEX: Market = Market::Custom(APEX_MARKET_ID);

pub const APEX_BASE_URL: &str = "https://omni.apex.exchange";
pub const APEX_SYMBOLS: &str = "/api/v3/symbols";
pub const APEX_ALL_TICKERS: &str = "/api/v3/data/all-ticker-info";
pub const APEX_DEPTH: &str = "/api/v3/depth";

pub const APEX_WS: &str = "wss://quote.omni.apex.exchange/realtime_public?v=2";
/// ApeX closes a connection about 150 s after its last client `pong`, however busy the stream is.
pub const APEX_WS_PONG_INTERVAL: Duration = Duration::from_secs(30);

pub const APEX_MAX_BOOK_LEVELS: usize = 200;
