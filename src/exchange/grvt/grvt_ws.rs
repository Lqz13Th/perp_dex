use extrema_infra::prelude::{
    LobParam, LobWsDecoder, TaskEvent, WsChannel, WsFrameRunner, decode_raw_ws,
};

use super::{
    config_assets::GRVT_MARKET_ID,
    grvt_ws_msg::GrvtWsData,
    schemas::ws::{
        lob::{WsBookGrvt, WsMiniTickerGrvt},
        trades::WsTradeGrvt,
    },
};

/// Register with `EnvBuilder::with_ws_decoder(GrvtWs)` and declare tasks on [`GRVT`](super::config_assets::GRVT).
#[derive(Clone, Copy, Debug, Default)]
pub struct GrvtWs;

impl LobWsDecoder for GrvtWs {
    const ID: u16 = GRVT_MARKET_ID;
    const NAME: &'static str = "grvt";

    async fn ws_channel<R: WsFrameRunner>(&self, channel: &WsChannel, runner: R) {
        match channel {
            WsChannel::Trades(_) => {
                runner
                    .ws_loop(TaskEvent::Trade, GrvtWsData::<WsTradeGrvt>::decode_trades)
                    .await;
            },
            WsChannel::Lob(Some(LobParam::Bbo { .. })) => {
                runner
                    .ws_loop(
                        TaskEvent::Lob,
                        GrvtWsData::<WsMiniTickerGrvt>::decode_single,
                    )
                    .await;
            },
            WsChannel::Lob(_) => {
                runner
                    .ws_loop(TaskEvent::Lob, GrvtWsData::<WsBookGrvt>::decode_single)
                    .await;
            },
            WsChannel::Other(_) => {
                runner.ws_loop(TaskEvent::WsOther, decode_raw_ws).await;
            },
            _ => {},
        }
    }
}
