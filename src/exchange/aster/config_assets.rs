use extrema_infra::prelude::Market;

pub const ASTER_MARKET_ID: u16 = 2;
pub const ASTER: Market = Market::Custom(ASTER_MARKET_ID);

pub const ASTER_BASE_URL: &str = "https://fapi.asterdex.com";
pub const ASTER_EXCHANGE_INFO: &str = "/fapi/v3/exchangeInfo";
pub const ASTER_DEPTH: &str = "/fapi/v3/depth";
pub const ASTER_TICKER_PRICE: &str = "/fapi/v3/ticker/price";
pub const ASTER_PREMIUM_INDEX: &str = "/fapi/v3/premiumIndex";
pub const ASTER_FUNDING_INFO: &str = "/fapi/v3/fundingInfo";

pub const ASTER_WS: &str = "wss://fstream.asterdex.com/ws";
