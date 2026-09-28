use extrema_infra::prelude::{
    LobParam, LobWsDecoder, TaskEvent, WsChannel, WsFrameRunner, decode_raw_ws,
};

use crate::exchange::ws_keepalive::WsKeepalive;

use super::{
    config_assets::{
        LIGHTER_MARKET_ID, LIGHTER_RH_MARKET_ID, LIGHTER_WS_PING, LIGHTER_WS_PING_INTERVAL,
    },
    lighter_ws_msg::LighterWsData,
    schemas::ws::{
        lob::{WsOrderBookLighter, WsTickerLighter},
        trades::WsTradeLighter,
    },
};

/// Register with `EnvBuilder::with_ws_decoder(LighterWs)` and declare tasks on [`LIGHTER`](super::config_assets::LIGHTER).
#[derive(Clone, Copy, Debug, Default)]
pub struct LighterWs;

/// Register with `EnvBuilder::with_ws_decoder(LighterRhWs)` and declare tasks on [`LIGHTER_RH`](super::config_assets::LIGHTER_RH).
#[derive(Clone, Copy, Debug, Default)]
pub struct LighterRhWs;

impl LobWsDecoder for LighterWs {
    const ID: u16 = LIGHTER_MARKET_ID;
    const NAME: &'static str = "lighter";

    async fn ws_channel<R: WsFrameRunner>(&self, channel: &WsChannel, runner: R) {
        lighter_ws_channel::<LIGHTER_MARKET_ID, R>(channel, runner).await;
    }
}

impl LobWsDecoder for LighterRhWs {
    const ID: u16 = LIGHTER_RH_MARKET_ID;
    const NAME: &'static str = "lighter_rh";

    async fn ws_channel<R: WsFrameRunner>(&self, channel: &WsChannel, runner: R) {
        lighter_ws_channel::<LIGHTER_RH_MARKET_ID, R>(channel, runner).await;
    }
}

async fn lighter_ws_channel<const ID: u16, R: WsFrameRunner>(channel: &WsChannel, runner: R) {
    match channel {
        WsChannel::Trades(_) => {
            runner
                .ws_loop(
                    TaskEvent::Trade,
                    LighterWsData::<WsTradeLighter<ID>>::decode_trades,
                )
                .await;
        },
        WsChannel::Lob(Some(LobParam::Bbo { .. })) => {
            runner
                .ws_loop(
                    TaskEvent::Lob,
                    LighterWsData::<WsTickerLighter<ID>>::decode_single,
                )
                .await;
        },
        WsChannel::Lob(_) => {
            runner
                .ws_loop(
                    TaskEvent::Lob,
                    LighterWsData::<WsOrderBookLighter<ID>>::decode_single,
                )
                .await;
        },
        WsChannel::Other(_) => {
            runner.ws_loop(TaskEvent::WsOther, decode_raw_ws).await;
        },
        _ => {},
    }
}

/// Lighter closes a connection after two minutes without a client frame.
pub fn lighter_keepalive() -> WsKeepalive {
    WsKeepalive::new(|| LIGHTER_WS_PING.into(), LIGHTER_WS_PING_INTERVAL)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[test]
    fn ping_interval_is_well_inside_the_two_minute_limit() {
        assert!(LIGHTER_WS_PING_INTERVAL <= Duration::from_secs(60));
        assert!(lighter_keepalive().due(std::time::Instant::now()));
    }
}
