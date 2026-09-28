use extrema_infra::prelude::{
    LobParam, LobWsDecoder, TaskEvent, WsChannel, WsFrameRunner, decode_raw_ws,
};

use super::{
    arcus_ws_msg::ArcusWsData,
    config_assets::ARCUS_MARKET_ID,
    schemas::ws::{
        lob::{WsBboArcus, WsL2BookArcus},
        trades::WsTradeArcus,
    },
};

/// Register with `EnvBuilder::with_ws_decoder(ArcusWs)` and declare tasks on [`ARCUS`](super::config_assets::ARCUS).
#[derive(Clone, Copy, Debug, Default)]
pub struct ArcusWs;

impl LobWsDecoder for ArcusWs {
    const ID: u16 = ARCUS_MARKET_ID;
    const NAME: &'static str = "arcus";

    async fn ws_channel<R: WsFrameRunner>(&self, channel: &WsChannel, runner: R) {
        match channel {
            WsChannel::Trades(_) => {
                runner
                    .ws_loop(TaskEvent::Trade, ArcusWsData::<WsTradeArcus>::decode_trades)
                    .await;
            },
            WsChannel::Lob(Some(LobParam::Bbo { .. })) => {
                runner
                    .ws_loop(TaskEvent::Lob, ArcusWsData::<WsBboArcus>::decode_single)
                    .await;
            },
            WsChannel::Lob(_) => {
                runner
                    .ws_loop(TaskEvent::Lob, ArcusWsData::<WsL2BookArcus>::decode_single)
                    .await;
            },
            WsChannel::Other(_) => {
                runner.ws_loop(TaskEvent::WsOther, decode_raw_ws).await;
            },
            _ => {},
        }
    }
}
