//! Lighter private websocket on a live account (`LIGHTER_ACCOUNT_INDEX`, `LIGHTER_API_KEY_INDEX`,
//! `LIGHTER_API_PRIVATE_KEY`): order, position and fill streams, plus order entry over the websocket. Posts a
//! post-only bid 5% below the touch (it cannot fill), moves it 1% lower, then cancels it.
//!
//! `cargo run --example lighter_private_ws --features lighter -- @139`

use std::{collections::HashMap, sync::Arc, time::Duration};

use extrema_infra::{
    arch::market_assets::api_general::{CancelOrderParams, OrderParams, get_mills_timestamp},
    prelude::*,
};
use perp_dex::prelude::*;
use tokio::sync::oneshot;
use tracing::{Level, warn};

const ORDERS: u64 = 1;
const POSITIONS: u64 = 2;
const FILLS: u64 = 3;
const TX: u64 = 4;
const TICK: u64 = 10;

fn channels() -> [(u64, WsChannel); 4] {
    [
        (ORDERS, WsChannel::AccountOrders),
        (POSITIONS, WsChannel::AccountPositions),
        (FILLS, WsChannel::Other(LIGHTER_WS_ACCOUNT_TRADES.into())),
        (TX, WsChannel::Other(LIGHTER_TX_CHANNEL.into())),
    ]
}

#[derive(Clone)]
struct LighterPrivateWs {
    registry: Arc<CommandRegistry>,
    cli: LighterCli,
    inst: String,
    size: String,
    price: f64,
    cli_order_id: String,
    keepalive: HashMap<u64, WsKeepalive>,
    tx_ready: bool,
    ticks: u64,
}

impl LighterPrivateWs {
    async fn send_tx(&self, id: &str, tx: InfraResult<SignedLighterTx>) {
        let Some(handle) = self.find_ws_handle(&WsChannel::Other(LIGHTER_TX_CHANNEL.into()), TX)
        else {
            return;
        };
        let tx = match tx {
            Ok(tx) => tx,
            Err(e) => return warn!("{id}: signing failed: {e:?}"),
        };
        println!("-> {id} (tx {})", tx.tx_hash);
        let msg = lighter_ws_send_tx_msg(id, &tx);
        if let Err(e) = handle
            .send_command(
                TaskCommand::WsMessage {
                    msg,
                    ack: AckHandle::none(),
                },
                None,
            )
            .await
        {
            warn!("{id}: send failed: {e:?}");
        }
    }

    fn price(&self, factor: f64) -> String {
        let scale = self
            .cli
            .market_scale(cli_to_lighter_market_id(&self.inst).unwrap())
            .unwrap();
        format!(
            "{:.prec$}",
            self.price * factor,
            prec = scale.price_decimals as usize
        )
    }
}

impl Strategy for LighterPrivateWs {
    async fn initialize(&mut self) {}
}

impl CommandEmitter for LighterPrivateWs {
    fn command_init(&mut self, registry: Arc<CommandRegistry>) {
        self.registry = registry;
    }

    fn command_registry(&self) -> Arc<CommandRegistry> {
        self.registry.clone()
    }
}

impl EventHandler for LighterPrivateWs {
    async fn on_ws_event(&mut self, msg: InfraMsg<WsTaskInfo>) {
        let channel = &msg.data.ws_channel;
        let Some(handle) = self.find_ws_handle(channel, msg.task_id) else {
            return;
        };
        let connect = async {
            let (tx, rx) = oneshot::channel();
            let url = self.cli.get_private_connect_msg(channel).await?;
            handle
                .send_command(
                    TaskCommand::WsConnect {
                        msg: url,
                        ack: AckHandle::new(tx),
                    },
                    Some((AckStatus::WsConnect, rx)),
                )
                .await?;
            if msg.task_id == TX {
                return Ok(());
            }
            let sub = self.cli.get_private_sub_msg(channel).await?;
            handle
                .send_command(
                    TaskCommand::WsMessage {
                        msg: sub,
                        ack: AckHandle::none(),
                    },
                    None,
                )
                .await
        };
        match connect.await {
            Ok(()) => {
                println!("connected task {} {channel:?}", msg.task_id);
                self.tx_ready |= msg.task_id == TX;
            },
            Err(e) => warn!("task {} connect failed: {e:?}", msg.task_id),
        }
    }

    async fn on_acc_order(&mut self, msg: InfraMsg<Vec<WsAccOrder>>) {
        for o in msg.data.iter() {
            println!(
                "<- order {} {:?} {:?} {} @ {} filled {} {:?} (id {:?}, cli {:?})",
                o.inst,
                o.side,
                o.order_type,
                o.size,
                o.price,
                o.filled_size,
                o.status,
                o.order_id,
                o.cli_order_id
            );
        }
    }

    async fn on_acc_pos(&mut self, msg: InfraMsg<Vec<WsAccPosition>>) {
        for p in msg
            .data
            .iter()
            .filter(|p| p.size != 0.0 || p.inst == self.inst)
        {
            println!(
                "<- position {} {} @ {} {:?}",
                p.inst, p.size, p.avg_price, p.margin_mode
            );
        }
    }

    async fn on_ws_other(&mut self, msg: InfraMsg<Vec<WsOtherMessage>>) {
        for m in msg.data.iter() {
            match msg.task_id {
                FILLS => {
                    let Ok(frame) = serde_json::from_str::<WsAccountTradesLighter>(&m.raw_json)
                    else {
                        continue;
                    };
                    let account = self.cli.auth.as_ref().map_or(0, |a| a.account_index);
                    for f in frame.into_fills(account) {
                        println!(
                            "<- fill {} {:?} {} @ {} maker {} (order {})",
                            f.inst, f.side, f.size, f.price, f.is_maker, f.order_id
                        );
                    }
                },
                TX => match serde_json::from_str::<WsSendTxLighter>(&m.raw_json) {
                    Ok(WsSendTxLighter::Sent { id, tx_hash, .. }) => {
                        println!("<- {id:?} accepted {tx_hash:?}");
                    },
                    Ok(WsSendTxLighter::Rejected { id, error }) => {
                        println!("<- {id:?} rejected {} {}", error.code, error.message);
                        self.cli.invalidate_nonce();
                    },
                    Err(_) => {},
                },
                _ => {},
            }
        }
    }

    async fn on_schedule(&mut self, _msg: InfraMsg<AltScheduleEvent>) {
        for (task_id, channel) in channels() {
            let handle = self.find_ws_handle(&channel, task_id);
            if let (Some(keepalive), Some(handle)) = (self.keepalive.get_mut(&task_id), handle)
                && let Err(e) = keepalive.on_frame(&handle).await
            {
                warn!("task {task_id} keepalive failed: {e:?}");
            }
        }

        if !self.tx_ready {
            return;
        }
        self.ticks += 1;
        match self.ticks {
            3 => {
                let order = OrderParams {
                    inst: self.inst.clone(),
                    side: OrderSide::BUY,
                    size: self.size.clone(),
                    order_type: OrderType::PostOnly,
                    price: Some(self.price(1.0)),
                    reduce_only: Some(false),
                    margin_mode: None,
                    position_side: None,
                    time_in_force: None,
                    client_order_id: Some(self.cli_order_id.clone()),
                    extra: Default::default(),
                };
                let tx = self
                    .cli
                    .sign_orders(&[order])
                    .await
                    .map(|mut t| t.remove(0));
                self.send_tx("place", tx).await;
            },
            6 => {
                let tx = self
                    .cli
                    .sign_modify(
                        &self.inst,
                        None,
                        Some(&self.cli_order_id),
                        &self.size,
                        &self.price(0.99),
                    )
                    .await;
                self.send_tx("modify", tx).await;
            },
            9 => {
                let cancel = CancelOrderParams {
                    inst: self.inst.clone(),
                    order_id: None,
                    cli_order_id: Some(self.cli_order_id.clone()),
                };
                let tx = self
                    .cli
                    .sign_cancels(&[cancel])
                    .await
                    .map(|mut t| t.remove(0));
                self.send_tx("cancel", tx).await;
            },
            _ => {},
        }
    }
}

#[tokio::main]
async fn main() -> InfraResult<()> {
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();
    let inst = std::env::args().nth(1).unwrap_or_else(|| "@139".into());

    let mut cli = LighterCli::default();
    cli.init_api_key();
    cli.init_market_scales().await?;
    let scale = cli.market_scale(cli_to_lighter_market_id(&inst)?)?;
    let bid = cli
        .get_orderbook(&inst, InstrumentType::Perpetual, 1)
        .await?
        .bids
        .first()
        .map(|l| l.0)
        .ok_or_else(|| InfraError::Msg(format!("no bid on {inst}")))?;
    let price = bid * 0.95;
    let lot = 10f64.powi(-(scale.size_decimals as i32));
    let size = format!(
        "{:.prec$}",
        ((11.0 / price) / lot).ceil() * lot,
        prec = scale.size_decimals as usize
    );
    println!("bid {bid}: post-only {size} @ ~{price:.2}");
    let cleanup = cli.clone();
    let cli_order_id = (get_mills_timestamp() % 1_000_000_000).to_string();

    let mut builder = EnvBuilder::new().with_ws_decoder(LighterWs);
    for (task_id, ws_channel) in channels() {
        builder = builder.with_task(WsTaskInfo {
            market: LIGHTER,
            ws_channel,
            filter_channels: false,
            chunk: 1,
            task_base_id: Some(task_id),
        });
    }
    let env = builder
        .with_task(AltTaskInfo {
            alt_task_type: AltTaskType::TimeScheduler(Duration::from_secs(1)),
            chunk: 1,
            task_base_id: Some(TICK),
        })
        .with_strategy_module(LighterPrivateWs {
            registry: Arc::new(CommandRegistry::default()),
            cli,
            inst: inst.clone(),
            size,
            price,
            cli_order_id: cli_order_id.clone(),
            keepalive: channels()
                .into_iter()
                .map(|(id, _)| (id, lighter_keepalive()))
                .collect(),
            tx_ready: false,
            ticks: 0,
        })
        .build()?;

    tokio::select! {
        _ = env.execute() => {},
        _ = tokio::time::sleep(Duration::from_secs(25)) => {},
    }

    let left = cleanup.get_open_orders(&inst, None).await?;
    for o in left
        .iter()
        .filter(|o| o.cli_order_id.as_deref() == Some(cli_order_id.as_str()))
    {
        println!("still open, cancelling over REST: {}", o.order_id);
        cleanup.cancel_order(&inst, Some(&o.order_id), None).await?;
    }
    Ok(())
}
