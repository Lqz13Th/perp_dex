use extrema_infra::prelude::{
    LobParam, LobWsDecoder, TaskEvent, WsChannel, WsFrameRunner, decode_raw_ws,
};

use super::{
    config_assets::EDGEX_MARKET_ID,
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
