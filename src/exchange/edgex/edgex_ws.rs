use extrema_infra::{
    arch::market_assets::api_general::get_mills_timestamp,
    prelude::{LobParam, LobWsDecoder, TaskEvent, WsChannel, WsFrameRunner, decode_raw_ws},
};

use crate::exchange::ws_keepalive::WsKeepalive;

use super::{
    api_utils::ws_pong_msg_edgex,
    config_assets::{EDGEX_MARKET_ID, EDGEX_WS_PONG_INTERVAL},
    edgex_ws_msg::EdgexWsData,
    schemas::ws::{
        lob::{WsBookTickerEdgex, WsDepthEdgex},
        trades::WsTradeEdgex,
    },
};

/// Register with `EnvBuilder::with_ws_decoder(EdgexWs)` and declare tasks on [`EDGEX`](super::config_assets::EDGEX).
#[derive(Clone, Copy, Debug, Default)]
pub struct EdgexWs;

impl LobWsDecoder for EdgexWs {
    const ID: u16 = EDGEX_MARKET_ID;
    const NAME: &'static str = "edgex";

    async fn ws_channel<R: WsFrameRunner>(&self, channel: &WsChannel, runner: R) {
        match channel {
            WsChannel::Trades(_) => {
                runner
                    .ws_loop(TaskEvent::Trade, EdgexWsData::<WsTradeEdgex>::decode_trades)
                    .await;
            },
            WsChannel::Lob(Some(LobParam::Bbo { .. })) => {
                runner
                    .ws_loop(
                        TaskEvent::Lob,
                        EdgexWsData::<WsBookTickerEdgex>::decode_batch,
                    )
                    .await;
            },
            WsChannel::Lob(_) => {
                runner
                    .ws_loop(TaskEvent::Lob, EdgexWsData::<WsDepthEdgex>::decode_single)
                    .await;
            },
            WsChannel::Other(_) => {
                runner.ws_loop(TaskEvent::WsOther, decode_raw_ws).await;
            },
            _ => {},
        }
    }
}

/// edgeX pings every 10 s and closes a connection about a minute after the
/// client's last `pong`, however busy the stream; a client `ping` does not count.
/// Its pings reach `on_lob` / `on_trade` as empty batches, so quiet streams still
/// call [`WsKeepalive::on_frame`].
pub fn edgex_keepalive() -> WsKeepalive {
    WsKeepalive::new(
        || ws_pong_msg_edgex(get_mills_timestamp()),
        EDGEX_WS_PONG_INTERVAL,
    )
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn pong_interval_is_well_inside_the_pong_window() {
        assert!(EDGEX_WS_PONG_INTERVAL <= Duration::from_secs(30));
        assert!(edgex_keepalive().due(Instant::now()));
    }
}
