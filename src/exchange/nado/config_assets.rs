use extrema_infra::prelude::Market;

pub const NADO_MARKET_ID: u16 = 9;
pub const NADO: Market = Market::Custom(NADO_MARKET_ID);

pub const NADO_BASE_URL: &str = "https://api.prod.nado.xyz";
pub const NADO_GATEWAY_QUERY: &str = "/gateway/v1/query";
pub const NADO_ARCHIVE_TICKERS: &str = "/archive/v2/tickers";
pub const NADO_ARCHIVE_CONTRACTS: &str = "/archive/v2/contracts";

/// The Cloudflare subscription endpoints refuse clients without `permessage-deflate`,
/// which infra's websocket does not negotiate; the direct endpoint serves the same
/// streams uncompressed.
pub const NADO_WS: &str = "wss://direct-gateway.prod.nado-backend.xyz/v1/subscribe";

pub const NADO_MAX_BOOK_LEVELS: usize = 100;
