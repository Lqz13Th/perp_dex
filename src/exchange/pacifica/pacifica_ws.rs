use extrema_infra::prelude::{
    LobParam, LobWsDecoder, TaskEvent, WsChannel, WsFrameRunner, decode_raw_ws,
};

use crate::exchange::ws_keepalive::WsKeepalive;

use super::{
    config_assets::{PACIFICA_MARKET_ID, PACIFICA_WS_PING, PACIFICA_WS_PING_INTERVAL},
    pacifica_ws_msg::PacificaWsData,
    schemas::ws::{
        lob::{WsBboPacifica, WsBookPacifica},
        trades::WsTradePacifica,
    },
};

/// Register with `EnvBuilder::with_ws_decoder(PacificaWs)` and declare tasks on [`PACIFICA`](super::config_assets::PACIFICA).
#[derive(Clone, Copy, Debug, Default)]
pub struct PacificaWs;

impl LobWsDecoder for PacificaWs {
    const ID: u16 = PACIFICA_MARKET_ID;
    const NAME: &'static str = "pacifica";

    async fn ws_channel<R: WsFrameRunner>(&self, channel: &WsChannel, runner: R) {
        match channel {
            WsChannel::Trades(_) => {
                runner
                    .ws_loop(
                        TaskEvent::Trade,
                        PacificaWsData::<WsTradePacifica>::decode_batch,
                    )
                    .await;
            },
            WsChannel::Lob(Some(LobParam::Bbo { .. })) => {
                runner
                    .ws_loop(
                        TaskEvent::Lob,
                        PacificaWsData::<WsBboPacifica>::decode_single,
                    )
                    .await;
            },
            WsChannel::Lob(_) => {
                runner
                    .ws_loop(
                        TaskEvent::Lob,
                        PacificaWsData::<WsBookPacifica>::decode_single,
                    )
                    .await;
            },
            WsChannel::Other(_) => {
                runner.ws_loop(TaskEvent::WsOther, decode_raw_ws).await;
            },
            _ => {},
        }
    }
}

/// Pacifica closes a connection after 60 seconds without a client frame.
pub fn pacifica_keepalive() -> WsKeepalive {
    WsKeepalive::new(|| PACIFICA_WS_PING.into(), PACIFICA_WS_PING_INTERVAL)
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    #[test]
    fn ping_interval_is_well_inside_the_one_minute_limit() {
        assert!(PACIFICA_WS_PING_INTERVAL <= Duration::from_secs(30));
        assert_eq!(PACIFICA_WS_PING, r#"{"method":"ping"}"#);
        assert!(pacifica_keepalive().due(Instant::now()));
    }
}
