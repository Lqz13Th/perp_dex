use std::time::Duration;

use extrema_infra::prelude::Market;

pub const PACIFICA_MARKET_ID: u16 = 10;
pub const PACIFICA: Market = Market::Custom(PACIFICA_MARKET_ID);

/// Perps are USDC-margined; `/info` names them by base asset only.
pub const PACIFICA_QUOTE: &str = "USDC";

pub const PACIFICA_BASE_URL: &str = "https://api.pacifica.fi";
pub const PACIFICA_INFO: &str = "/api/v1/info";
pub const PACIFICA_PRICES: &str = "/api/v1/info/prices";
pub const PACIFICA_BOOK: &str = "/api/v1/book";

pub const PACIFICA_WS: &str = "wss://ws.pacifica.fi/ws";
/// Pacifica drops a connection after 60 seconds without a client frame, however busy the stream is.
pub const PACIFICA_WS_PING: &str = r#"{"method":"ping"}"#;
pub const PACIFICA_WS_PING_INTERVAL: Duration = Duration::from_secs(30);

pub const PACIFICA_MAX_BOOK_LEVELS: usize = 10;
pub const PACIFICA_BOOK_AGG_LEVEL: u16 = 1;
