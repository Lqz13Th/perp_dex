use extrema_infra::{
    arch::market_assets::api_general::get_mills_timestamp,
    prelude::{LobWsDecoder, TaskEvent, WsChannel, WsFrameRunner, decode_raw_ws},
};

use crate::exchange::ws_keepalive::WsKeepalive;

use super::{
    apex_ws_msg::ApexWsData,
    api_utils::ws_pong_msg_apex,
    config_assets::{APEX_MARKET_ID, APEX_WS_PONG_INTERVAL},
    schemas::ws::{lob::WsOrderBookApex, trades::WsTradeApex},
};

/// Register with `EnvBuilder::with_ws_decoder(ApexWs)` and declare tasks on [`APEX`](super::config_assets::APEX).
#[derive(Clone, Copy, Debug, Default)]
pub struct ApexWs;

impl LobWsDecoder for ApexWs {
    const ID: u16 = APEX_MARKET_ID;
    const NAME: &'static str = "apex";

    async fn ws_channel<R: WsFrameRunner>(&self, channel: &WsChannel, runner: R) {
        match channel {
            WsChannel::Trades(_) => {
                runner
                    .ws_loop(TaskEvent::Trade, ApexWsData::<WsTradeApex>::decode_batch)
                    .await;
            },
            WsChannel::Lob(_) => {
                runner
                    .ws_loop(TaskEvent::Lob, ApexWsData::<WsOrderBookApex>::decode_single)
                    .await;
            },
            WsChannel::Other(_) => {
                runner.ws_loop(TaskEvent::WsOther, decode_raw_ws).await;
            },
            _ => {},
        }
    }
}

/// ApeX pings every 15 s and closes a connection about 150 s after the client's
/// last `pong`, however busy the stream; its pings reach `on_lob` / `on_trade` as
/// empty batches, so quiet streams still call [`WsKeepalive::on_frame`].
pub fn apex_keepalive() -> WsKeepalive {
    WsKeepalive::new(
        || ws_pong_msg_apex(get_mills_timestamp()),
        APEX_WS_PONG_INTERVAL,
    )
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn pong_interval_is_well_inside_the_pong_window() {
        assert!(APEX_WS_PONG_INTERVAL <= Duration::from_secs(60));
        assert!(apex_keepalive().due(Instant::now()));
    }
}
