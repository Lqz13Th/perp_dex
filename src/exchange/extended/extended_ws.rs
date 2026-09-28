use extrema_infra::prelude::{LobWsDecoder, TaskEvent, WsChannel, WsFrameRunner, decode_raw_ws};

use super::{
    config_assets::EXTENDED_MARKET_ID,
    extended_ws_msg::ExtendedWsData,
    schemas::ws::{lob::WsOrderBookExtended, trades::WsTradeExtended},
};

/// Register with `EnvBuilder::with_ws_decoder(ExtendedWs)` and declare tasks on [`EXTENDED`](super::config_assets::EXTENDED).
///
/// Connect each task with [`ExtendedCli::get_public_stream_target`](super::extended_cli::ExtendedCli::get_public_stream_target)
/// and send nothing after it; the URL is the subscription.
#[derive(Clone, Copy, Debug, Default)]
pub struct ExtendedWs;

impl LobWsDecoder for ExtendedWs {
    const ID: u16 = EXTENDED_MARKET_ID;
    const NAME: &'static str = "extended";

    async fn ws_channel<R: WsFrameRunner>(&self, channel: &WsChannel, runner: R) {
        match channel {
            WsChannel::Trades(_) => {
                runner
                    .ws_loop(
                        TaskEvent::Trade,
                        ExtendedWsData::<WsTradeExtended>::decode_trades,
                    )
                    .await;
            },
            WsChannel::Lob(_) => {
                runner
                    .ws_loop(
                        TaskEvent::Lob,
                        ExtendedWsData::<WsOrderBookExtended>::decode_single,
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
