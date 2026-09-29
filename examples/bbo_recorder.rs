//! Records every BBO update and trade of a set of stocks from every venue with a BBO stream.
//!
//! `cargo run --release --example bbo_recorder -- NVDA,TSLA 600 /tmp/perp_dex_bbo`
//!
//! `bbo-<start>.csv`: `recv_us,venue,symbol,exch_us,bid,bid_size,ask,ask_size`.
//! `trades-<start>.csv`: `recv_us,venue,symbol,exch_us,taker_side,price,size,trade_id`.

use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{BufWriter, Write},
    sync::{Arc, Mutex},
    time::Duration,
};

use extrema_infra::{
    arch::market_assets::{
        api_data::utils_data::InstrumentInfo, api_general::get_micros_timestamp,
        exchange::prelude::HyperliquidCli,
    },
    prelude::*,
};
use perp_dex::prelude::*;
use tokio::sync::oneshot;
use tracing::{Level, warn};

const DEFAULT_SYMBOLS: &str =
    "NVDA,TSLA,AAPL,MSFT,GOOGL,AMZN,META,AMD,MU,SNDK,HOOD,COIN,CRCL,MSTR,PLTR,INTC,ORCL";
const FLUSH_TICK: u64 = 100_000;

type Writer = Arc<Mutex<BufWriter<File>>>;

#[derive(Clone)]
struct VenueTask {
    label: &'static str,
    cli: PerpDexClients,
    channel: WsChannel,
    symbols: HashMap<String, String>,
    keepalive: Option<WsKeepalive>,
    rows: u64,
    connects: u64,
}

#[derive(Clone)]
struct BboRecorder {
    registry: Arc<CommandRegistry>,
    tasks: BTreeMap<u64, VenueTask>,
    bbo_out: Writer,
    trades_out: Writer,
    started_us: u64,
}

fn bbo_channel() -> WsChannel {
    WsChannel::Lob(Some(LobParam::Bbo { frequency: None }))
}

fn trades_channel() -> WsChannel {
    WsChannel::Trades(None)
}

async fn connect_and_subscribe(task: &VenueTask, handle: &CommandHandle) -> InfraResult<()> {
    let channel = &task.channel;
    let (tx, rx) = oneshot::channel();
    let ack = Some((AckStatus::WsConnect, rx));

    if let PerpDexClients::Extended(cli) = &task.cli {
        let inst = task
            .symbols
            .keys()
            .next()
            .expect("one Extended market per task");
        let target = cli.get_public_stream_target(channel, Some(inst))?;
        return handle
            .send_command(
                TaskCommand::WsConnectWithTarget {
                    target,
                    ack: AckHandle::new(tx),
                },
                ack,
            )
            .await;
    }

    let url = task.cli.get_public_connect_msg(channel).await?;
    handle
        .send_command(
            TaskCommand::WsConnect {
                msg: url,
                ack: AckHandle::new(tx),
            },
            ack,
        )
        .await?;

    let all_markets = matches!(task.cli, PerpDexClients::Edgex(_)) && *channel == bbo_channel();
    let subs = if all_markets {
        vec![task.cli.get_public_sub_msg(channel, None).await?]
    } else {
        let mut subs = Vec::with_capacity(task.symbols.len());
        for inst in task.symbols.keys() {
            subs.push(
                task.cli
                    .get_public_sub_msg(channel, Some(std::slice::from_ref(inst)))
                    .await?,
            );
        }
        subs
    };
    for msg in subs {
        handle
            .send_command(
                TaskCommand::WsMessage {
                    msg,
                    ack: AckHandle::none(),
                },
                None,
            )
            .await?;
    }
    Ok(())
}

impl BboRecorder {
    async fn keepalive(&mut self, task_id: u64) -> Option<&mut VenueTask> {
        let channel = self.tasks.get(&task_id)?.channel.clone();
        let handle = self.find_ws_handle(&channel, task_id);
        let task = self.tasks.get_mut(&task_id)?;
        if let (Some(keepalive), Some(handle)) = (task.keepalive.as_mut(), handle)
            && let Err(e) = keepalive.on_frame(&handle).await
        {
            warn!("{} keepalive failed: {e:?}", task.label);
        }
        Some(task)
    }
}

impl Strategy for BboRecorder {
    async fn initialize(&mut self) {}
}

impl CommandEmitter for BboRecorder {
    fn command_init(&mut self, registry: Arc<CommandRegistry>) {
        self.registry = registry;
    }

    fn command_registry(&self) -> Arc<CommandRegistry> {
        self.registry.clone()
    }
}

impl EventHandler for BboRecorder {
    async fn on_ws_event(&mut self, msg: InfraMsg<WsTaskInfo>) {
        let (Some(handle), Some(task)) = (
            self.find_ws_handle(&msg.data.ws_channel, msg.task_id),
            self.tasks.get_mut(&msg.task_id),
        ) else {
            return;
        };

        task.connects += 1;
        if let Err(e) = connect_and_subscribe(task, &handle).await {
            warn!("{} subscribe failed: {e:?}", task.label);
        }
    }

    async fn on_lob(&mut self, msg: InfraMsg<Vec<WsLob>>) {
        let recv_us = get_micros_timestamp();
        let out = self.bbo_out.clone();
        let Some(task) = self.keepalive(msg.task_id).await else {
            return;
        };

        let mut out = out.lock().expect("writer lock");
        for lob in msg.data.iter() {
            let (Some(symbol), Some(bid), Some(ask)) = (
                task.symbols.get(&lob.inst),
                lob.bids.first(),
                lob.asks.first(),
            ) else {
                continue;
            };
            task.rows += 1;
            if let Err(e) = writeln!(
                out,
                "{recv_us},{},{symbol},{},{},{},{},{}",
                task.label, lob.timestamp, bid.price, bid.size, ask.price, ask.size
            ) {
                warn!("write failed: {e}");
            }
        }
    }

    async fn on_trade(&mut self, msg: InfraMsg<Vec<WsTrade>>) {
        let recv_us = get_micros_timestamp();
        let out = self.trades_out.clone();
        let Some(task) = self.keepalive(msg.task_id).await else {
            return;
        };

        let mut out = out.lock().expect("writer lock");
        for trade in msg.data.iter() {
            let Some(symbol) = task.symbols.get(&trade.inst) else {
                continue;
            };
            let side = match trade.side {
                OrderSide::BUY => "buy",
                OrderSide::SELL => "sell",
                _ => continue,
            };
            task.rows += 1;
            if let Err(e) = writeln!(
                out,
                "{recv_us},{},{symbol},{},{side},{},{},{}",
                task.label, trade.timestamp, trade.price, trade.size, trade.trade_id
            ) {
                warn!("write failed: {e}");
            }
        }
    }

    async fn on_schedule(&mut self, _msg: InfraMsg<AltScheduleEvent>) {
        for out in [&self.bbo_out, &self.trades_out] {
            if let Err(e) = out.lock().expect("writer lock").flush() {
                warn!("flush failed: {e}");
            }
        }

        let elapsed = (get_micros_timestamp() - self.started_us) / 1_000_000;
        if elapsed == 0 || !elapsed.is_multiple_of(60) {
            return;
        }
        let mut by_venue: BTreeMap<&str, [u64; 3]> = BTreeMap::new();
        for task in self.tasks.values() {
            let entry = by_venue.entry(task.label).or_default();
            let column = if task.channel == bbo_channel() { 0 } else { 1 };
            entry[column] += task.rows;
            entry[2] += task.connects;
        }
        println!("{elapsed}s");
        for (label, [bbo, trades, connects]) in by_venue {
            println!(
                "  {label:<11} bbo {:>7.1}/s  trades {:>6.2}/s  connects {connects}",
                bbo as f64 / elapsed as f64,
                trades as f64 / elapsed as f64,
            );
        }
    }
}

async fn instruments(cli: &PerpDexClients) -> Vec<InstrumentInfo> {
    match cli.get_instrument_info(InstrumentType::Perpetual).await {
        Ok(list) => list,
        Err(e) => {
            warn!("instrument list failed: {e:?}");
            Vec::new()
        },
    }
}

async fn venue_tasks(symbols: &[String]) -> InfraResult<Vec<VenueTask>> {
    let mut xyz = HyperliquidCli::default();
    xyz.set_perp_dex(Some("xyz".into()));
    xyz.init_inst_index_map().await?;
    let mut rh = LighterCli::default();
    rh.set_venue(LighterVenue::Robinhood);

    let venues: Vec<(&'static str, PerpDexClients, Option<WsKeepalive>)> = vec![
        ("hl_xyz", PerpDexClients::Hyperliquid(xyz), None),
        (
            "lighter",
            PerpDexClients::Lighter(LighterCli::default()),
            Some(lighter_keepalive()),
        ),
        (
            "lighter_rh",
            PerpDexClients::Lighter(rh),
            Some(lighter_keepalive()),
        ),
        ("aster", PerpDexClients::Aster(AsterCli::default()), None),
        ("arcus", PerpDexClients::Arcus(ArcusCli::default()), None),
        ("grvt", PerpDexClients::Grvt(GrvtCli::default()), None),
        (
            "extended",
            PerpDexClients::Extended(ExtendedCli::default()),
            None,
        ),
        (
            "edgex",
            PerpDexClients::Edgex(EdgexCli::default()),
            Some(edgex_keepalive()),
        ),
        ("nado", PerpDexClients::Nado(NadoCli::default()), None),
        (
            "pacifica",
            PerpDexClients::Pacifica(PacificaCli::default()),
            Some(pacifica_keepalive()),
        ),
    ];

    let mut tasks = Vec::new();
    for (label, cli, keepalive) in venues {
        let list = instruments(&cli).await;
        let code = |i: &InstrumentInfo, want: &str| i.inst_code.as_deref() == Some(want);
        let mut found = HashMap::new();
        for symbol in symbols {
            let hit = list.iter().find(|i| match label {
                "hl_xyz" => i.inst == format!("{symbol}_USDC_PERP"),
                "lighter" | "lighter_rh" => code(i, symbol),
                "aster" | "grvt" => i.inst == format!("{symbol}_USDT_PERP"),
                "arcus" => i.inst == format!("{symbol}_USD_PERP"),
                "extended" => i.inst.starts_with(&format!("{symbol}_")),
                "edgex" => code(i, &format!("{symbol}USDC")),
                "nado" => code(i, &format!("{symbol}-PERP")),
                "pacifica" => i.inst == format!("{symbol}_USDC_PERP"),
                _ => false,
            });
            if let Some(i) = hit {
                found.insert(i.inst.clone(), symbol.clone());
            }
        }
        println!("{label:<11} {} / {} symbols", found.len(), symbols.len());
        if found.is_empty() {
            continue;
        }

        for channel in [bbo_channel(), trades_channel()] {
            let task = |symbols| VenueTask {
                label,
                cli: cli.clone(),
                channel: channel.clone(),
                symbols,
                keepalive: keepalive.clone(),
                rows: 0,
                connects: 0,
            };
            if label == "extended" {
                tasks.extend(
                    found
                        .clone()
                        .into_iter()
                        .map(|(inst, symbol)| task(HashMap::from([(inst, symbol)]))),
                );
            } else {
                tasks.push(task(found.clone()));
            }
        }
    }
    Ok(tasks)
}

fn writer(path: &str, header: &str) -> InfraResult<Writer> {
    let mut out = BufWriter::with_capacity(1 << 20, File::create(path)?);
    writeln!(out, "{header}")?;
    Ok(Arc::new(Mutex::new(out)))
}

#[tokio::main]
async fn main() -> InfraResult<()> {
    tracing_subscriber::fmt().with_max_level(Level::WARN).init();
    let mut args = std::env::args().skip(1);
    let symbols: Vec<String> = args
        .next()
        .unwrap_or_else(|| DEFAULT_SYMBOLS.into())
        .split(',')
        .map(|s| s.trim().to_uppercase())
        .collect();
    let seconds = args.next().and_then(|a| a.parse().ok()).unwrap_or(600);
    let dir = args.next().unwrap_or_else(|| "/tmp/perp_dex_bbo".into());

    let tasks: BTreeMap<u64, VenueTask> = (1..).zip(venue_tasks(&symbols).await?).collect();
    std::fs::create_dir_all(&dir)?;
    let started_us = get_micros_timestamp();
    let stamp = started_us / 1_000_000;
    let bbo_out = writer(
        &format!("{dir}/bbo-{stamp}.csv"),
        "recv_us,venue,symbol,exch_us,bid,bid_size,ask,ask_size",
    )?;
    let trades_out = writer(
        &format!("{dir}/trades-{stamp}.csv"),
        "recv_us,venue,symbol,exch_us,taker_side,price,size,trade_id",
    )?;
    println!("{} tasks -> {dir}/{{bbo,trades}}-{stamp}.csv", tasks.len());

    let mut builder = EnvBuilder::new()
        .with_ws_decoder(LighterWs)
        .with_ws_decoder(LighterRhWs)
        .with_ws_decoder(AsterWs)
        .with_ws_decoder(ArcusWs)
        .with_ws_decoder(GrvtWs)
        .with_ws_decoder(ExtendedWs)
        .with_ws_decoder(EdgexWs)
        .with_ws_decoder(NadoWs)
        .with_ws_decoder(PacificaWs);
    for (task_id, task) in &tasks {
        builder = builder.with_task(WsTaskInfo {
            market: task.cli.market(),
            ws_channel: task.channel.clone(),
            filter_channels: true,
            chunk: 1,
            task_base_id: Some(*task_id),
        });
    }
    let env = builder
        .with_task(AltTaskInfo {
            alt_task_type: AltTaskType::TimeScheduler(Duration::from_secs(1)),
            chunk: 1,
            task_base_id: Some(FLUSH_TICK),
        })
        .with_strategy_module(BboRecorder {
            registry: Arc::new(CommandRegistry::default()),
            tasks,
            bbo_out: bbo_out.clone(),
            trades_out: trades_out.clone(),
            started_us,
        })
        .build()?;

    tokio::select! {
        _ = env.execute() => {},
        _ = tokio::time::sleep(Duration::from_secs(seconds)) => {},
    }
    for out in [&bbo_out, &trades_out] {
        out.lock().expect("writer lock").flush()?;
    }
    println!("done: {dir}/{{bbo,trades}}-{stamp}.csv");
    Ok(())
}
