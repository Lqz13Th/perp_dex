use extrema_infra::prelude::{
    LobParam, LobWsDecoder, TaskEvent, TradesParam, WsChannel, WsFrameRunner, decode_raw_ws,
};

use super::{
    aster_ws_msg::AsterWsData,
    config_assets::ASTER_MARKET_ID,
    schemas::ws::{
        lob::{WsBookTickerAster, WsDiffDepthAster, WsPartialDepthAster},
        trades::{WsAggTradeAster, WsTradeAster},
    },
};

/// Register with `EnvBuilder::with_ws_decoder(AsterWs)` and declare tasks on [`ASTER`](super::config_assets::ASTER).
#[derive(Clone, Copy, Debug, Default)]
pub struct AsterWs;

impl LobWsDecoder for AsterWs {
    const ID: u16 = ASTER_MARKET_ID;
    const NAME: &'static str = "aster";

    async fn ws_channel<R: WsFrameRunner>(&self, channel: &WsChannel, runner: R) {
        match channel {
            WsChannel::Trades(Some(TradesParam::AllTrades)) => {
                runner
                    .ws_loop(TaskEvent::Trade, AsterWsData::<WsTradeAster>::decode_single)
                    .await;
            },
            WsChannel::Trades(_) => {
                runner
                    .ws_loop(
                        TaskEvent::Trade,
                        AsterWsData::<WsAggTradeAster>::decode_single,
                    )
                    .await;
            },
            WsChannel::Lob(lob_param) => match lob_param {
                Some(LobParam::Bbo { .. }) => {
                    runner
                        .ws_loop(
                            TaskEvent::Lob,
                            AsterWsData::<WsBookTickerAster>::decode_single,
                        )
                        .await;
                },
                Some(LobParam::Snapshot { .. }) => {
                    runner
                        .ws_loop(
                            TaskEvent::Lob,
                            AsterWsData::<WsPartialDepthAster>::decode_single,
                        )
                        .await;
                },
                None | Some(LobParam::Incremental { .. }) => {
                    runner
                        .ws_loop(
                            TaskEvent::Lob,
                            AsterWsData::<WsDiffDepthAster>::decode_single,
                        )
                        .await;
                },
            },
            WsChannel::Other(_) => {
                runner.ws_loop(TaskEvent::WsOther, decode_raw_ws).await;
            },
            _ => {},
        }
    }
}
