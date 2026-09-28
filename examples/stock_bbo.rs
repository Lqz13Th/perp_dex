//! One stock's BBO from every venue with a per-market BBO stream on one `on_lob`.
//!
//! `cargo run --example stock_bbo -- NVDA 60`

use std::{collections::BTreeMap, sync::Arc, time::Duration};

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

const PRINT_TICK: u64 = 100;

#[derive(Clone)]
struct Venue {
    label: &'static str,
    cli: PerpDexClients,
    inst: String,
    keepalive: Option<WsKeepalive>,
}

#[derive(Clone, Default)]
struct Quote {
    bid: f64,
    ask: f64,
    exchange_ts_us: u64,
    updates: u64,
    connects: u64,
}

#[derive(Clone)]
struct StockBboBoard {
    registry: Arc<CommandRegistry>,
    venues: BTreeMap<u64, Venue>,
    quotes: BTreeMap<u64, Quote>,
}

fn bbo_channel() -> WsChannel {
    WsChannel::Lob(Some(LobParam::Bbo { frequency: None }))
}

async fn connect_and_subscribe(venue: &Venue, handle: &CommandHandle) -> InfraResult<()> {
    let channel = bbo_channel();
    let (tx, rx) = oneshot::channel();
    let ack = Some((AckStatus::WsConnect, rx));

    if let PerpDexClients::Extended(cli) = &venue.cli {
        let target = cli.get_public_stream_target(&channel, Some(&venue.inst))?;
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

    let url = venue.cli.get_public_connect_msg(&channel).await?;
    handle
        .send_command(
            TaskCommand::WsConnect {
                msg: url,
                ack: AckHandle::new(tx),
            },
            ack,
        )
        .await?;

    let insts = [venue.inst.clone()];
    let insts = (!matches!(venue.cli, PerpDexClients::Edgex(_))).then_some(&insts[..]);
    let sub = venue.cli.get_public_sub_msg(&channel, insts).await?;
    handle
        .send_command(
            TaskCommand::WsMessage {
                msg: sub,
                ack: AckHandle::none(),
            },
            None,
        )
        .await
}

impl Strategy for StockBboBoard {
    async fn initialize(&mut self) {}
}

impl CommandEmitter for StockBboBoard {
    fn command_init(&mut self, registry: Arc<CommandRegistry>) {
        self.registry = registry;
    }

    fn command_registry(&self) -> Arc<CommandRegistry> {
        self.registry.clone()
    }
}

impl EventHandler for StockBboBoard {
    async fn on_ws_event(&mut self, msg: InfraMsg<WsTaskInfo>) {
        let (Some(handle), Some(venue)) = (
            self.find_ws_handle(&msg.data.ws_channel, msg.task_id),
            self.venues.get(&msg.task_id),
        ) else {
            return;
        };

        self.quotes.entry(msg.task_id).or_default().connects += 1;
        if let Err(e) = connect_and_subscribe(venue, &handle).await {
            warn!("{} subscribe failed: {e:?}", venue.label);
        }
    }

    async fn on_lob(&mut self, msg: InfraMsg<Vec<WsLob>>) {
        let handle = self.find_ws_handle(&bbo_channel(), msg.task_id);
        let Some(venue) = self.venues.get_mut(&msg.task_id) else {
            return;
        };
        if let (Some(keepalive), Some(handle)) = (venue.keepalive.as_mut(), handle)
            && let Err(e) = keepalive.on_frame(&handle).await
        {
            warn!("{} keepalive failed: {e:?}", venue.label);
        }

        for lob in msg.data.iter().filter(|lob| lob.inst == venue.inst) {
            let (Some(bid), Some(ask)) = (lob.bids.first(), lob.asks.first()) else {
                continue;
            };
            let quote = self.quotes.entry(msg.task_id).or_default();
            quote.bid = bid.price;
            quote.ask = ask.price;
            quote.exchange_ts_us = lob.timestamp;
            quote.updates += 1;
        }
    }

    async fn on_schedule(&mut self, _msg: InfraMsg<AltScheduleEvent>) {
        let now_us = get_micros_timestamp();
        let mut mids: Vec<f64> = self
            .quotes
            .values()
            .filter(|q| q.bid > 0.0 && q.ask > 0.0)
            .map(|q| (q.bid + q.ask) / 2.0)
            .collect();
        mids.sort_by(f64::total_cmp);
        let median = mids.get(mids.len() / 2).copied().unwrap_or_default();

        println!(
            "{:<10} {:<20} {:>10} {:>10} {:>8} {:>8} {:>6} {:>8} {:>5}",
            "venue", "inst", "bid", "ask", "spr_bps", "dev_bps", "upd/s", "age_ms", "conn"
        );
        for (task_id, quote) in self.quotes.iter_mut() {
            let venue = &self.venues[task_id];
            let mid = (quote.bid + quote.ask) / 2.0;
            println!(
                "{:<10} {:<20} {:>10.3} {:>10.3} {:>8.2} {:>8.2} {:>6} {:>8.1} {:>5}",
                venue.label,
                venue.inst,
                quote.bid,
                quote.ask,
                (quote.ask - quote.bid) / mid * 1e4,
                (mid - median) / median * 1e4,
                quote.updates,
                (now_us as i64 - quote.exchange_ts_us as i64) as f64 / 1e3,
                quote.connects,
            );
            quote.updates = 0;
        }
        println!();
    }
}

async fn find_inst(
    cli: &PerpDexClients,
    matches: impl Fn(&InstrumentInfo) -> bool,
) -> InfraResult<Option<String>> {
    Ok(cli
        .get_instrument_info(InstrumentType::Perpetual)
        .await?
        .into_iter()
        .find(|i| matches(i))
        .map(|i| i.inst))
}

fn by_code(code: String) -> impl Fn(&InstrumentInfo) -> bool {
    move |i| i.inst_code.as_deref() == Some(code.as_str())
}

async fn venues(symbol: &str) -> InfraResult<Vec<Venue>> {
    let mut xyz = HyperliquidCli::default();
    xyz.set_perp_dex(Some("xyz".into()));
    xyz.init_inst_index_map().await?;
    let lighter = PerpDexClients::Lighter(LighterCli::default());
    let mut rh = LighterCli::default();
    rh.set_venue(LighterVenue::Robinhood);
    let lighter_rh = PerpDexClients::Lighter(rh);
    let grvt = PerpDexClients::Grvt(GrvtCli::default());
    let extended = PerpDexClients::Extended(ExtendedCli::default());
    let edgex = PerpDexClients::Edgex(EdgexCli::default());
    let nado = PerpDexClients::Nado(NadoCli::default());

    let grvt_inst = format!("{symbol}_USDT_PERP");
    let extended_prefix = format!("{symbol}_");
    let candidates = vec![
        (
            "hl_xyz",
            PerpDexClients::Hyperliquid(xyz),
            Some(format!("{symbol}_USDC_PERP")),
            None,
        ),
        (
            "lighter",
            lighter.clone(),
            find_inst(&lighter, by_code(symbol.into())).await?,
            Some(lighter_keepalive()),
        ),
        (
            "lighter_rh",
            lighter_rh.clone(),
            find_inst(&lighter_rh, by_code(symbol.into())).await?,
            Some(lighter_keepalive()),
        ),
        (
            "aster",
            PerpDexClients::Aster(AsterCli::default()),
            Some(format!("{symbol}_USDT_PERP")),
            None,
        ),
        (
            "arcus",
            PerpDexClients::Arcus(ArcusCli::default()),
            Some(format!("{symbol}_USD_PERP")),
            None,
        ),
        (
            "grvt",
            grvt.clone(),
            find_inst(&grvt, |i| i.inst == grvt_inst).await?,
            None,
        ),
        (
            "extended",
            extended.clone(),
            find_inst(&extended, |i| i.inst.starts_with(&extended_prefix)).await?,
            None,
        ),
        (
            "edgex",
            edgex.clone(),
            find_inst(&edgex, by_code(format!("{symbol}USDC"))).await?,
            None,
        ),
        (
            "nado",
            nado.clone(),
            find_inst(&nado, by_code(format!("{symbol}-PERP"))).await?,
            None,
        ),
        (
            "pacifica",
            PerpDexClients::Pacifica(PacificaCli::default()),
            Some(format!("{symbol}_USDC_PERP")),
            Some(pacifica_keepalive()),
        ),
    ];

    Ok(candidates
        .into_iter()
        .filter_map(|(label, cli, inst, keepalive)| {
            let Some(inst) = inst else {
                println!("{label} does not list {symbol}");
                return None;
            };
            Some(Venue {
                label,
                cli,
                inst,
                keepalive,
            })
        })
        .collect())
}

#[tokio::main]
async fn main() -> InfraResult<()> {
    tracing_subscriber::fmt().with_max_level(Level::WARN).init();
    let mut args = std::env::args().skip(1);
    let symbol = args.next().unwrap_or_else(|| "NVDA".into()).to_uppercase();
    let seconds = args.next().and_then(|a| a.parse().ok()).unwrap_or(30);

    let venues: BTreeMap<u64, Venue> = (1..).zip(venues(&symbol).await?).collect();

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
    for (task_id, venue) in &venues {
        builder = builder.with_task(WsTaskInfo {
            market: venue.cli.market(),
            ws_channel: bbo_channel(),
            filter_channels: true,
            chunk: 1,
            task_base_id: Some(*task_id),
        });
    }
    let env = builder
        .with_task(AltTaskInfo {
            alt_task_type: AltTaskType::TimeScheduler(Duration::from_secs(1)),
            chunk: 1,
            task_base_id: Some(PRINT_TICK),
        })
        .with_strategy_module(StockBboBoard {
            registry: Arc::new(CommandRegistry::default()),
            venues,
            quotes: BTreeMap::new(),
        })
        .build()?;

    tokio::select! {
        _ = env.execute() => {},
        _ = tokio::time::sleep(Duration::from_secs(seconds)) => {},
    }
    Ok(())
}
