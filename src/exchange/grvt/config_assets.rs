use extrema_infra::prelude::Market;

pub const GRVT_MARKET_ID: u16 = 5;
pub const GRVT: Market = Market::Custom(GRVT_MARKET_ID);

pub const GRVT_BASE_URL: &str = "https://market-data.grvt.io";
pub const GRVT_ALL_INSTRUMENTS: &str = "/full/v1/all_instruments";
pub const GRVT_MINI_TICKER: &str = "/full/v1/mini";
pub const GRVT_BOOK: &str = "/full/v1/book";

pub const GRVT_WS: &str = "wss://market-data.grvt.io/ws/full";

pub const GRVT_MAX_BOOK_LEVELS: usize = 500;
