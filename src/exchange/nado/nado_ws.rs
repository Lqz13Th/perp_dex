use extrema_infra::prelude::{
    LobParam, LobWsDecoder, TaskEvent, WsChannel, WsFrameRunner, decode_raw_ws,
};

use super::{
    config_assets::NADO_MARKET_ID,
    nado_ws_msg::NadoWsData,
    schemas::ws::{
        lob::{WsBboNado, WsBookDepthNado},
        trades::WsTradeNado,
    },
};

/// Register with `EnvBuilder::with_ws_decoder(NadoWs)` and declare tasks on [`NADO`](super::config_assets::NADO).
#[derive(Clone, Copy, Debug, Default)]
pub struct NadoWs;

impl LobWsDecoder for NadoWs {
    const ID: u16 = NADO_MARKET_ID;
    const NAME: &'static str = "nado";

    async fn ws_channel<R: WsFrameRunner>(&self, channel: &WsChannel, runner: R) {
        match channel {
            WsChannel::Trades(_) => {
                runner
                    .ws_loop(TaskEvent::Trade, NadoWsData::<WsTradeNado>::decode_single)
                    .await;
            },
            WsChannel::Lob(Some(LobParam::Bbo { .. })) => {
                runner
                    .ws_loop(TaskEvent::Lob, NadoWsData::<WsBboNado>::decode_single)
                    .await;
            },
            WsChannel::Lob(_) => {
                runner
                    .ws_loop(TaskEvent::Lob, NadoWsData::<WsBookDepthNado>::decode_single)
                    .await;
            },
            WsChannel::Other(_) => {
                runner.ws_loop(TaskEvent::WsOther, decode_raw_ws).await;
            },
            _ => {},
        }
    }
}
