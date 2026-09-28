//! Shared harness for the replay and live websocket tests.
//!
//! `replay` serves captured fixtures from a local websocket server and only
//! replays one when the subscribe message a task sends equals the one the live
//! venue accepted, or, for Extended, whose stream URL is the subscription, when
//! the task connects to the fixture's URL path. `live` connects every task to
//! its venue for a fixed time. Both run the real infra runtime with every
//! perp_dex decoder registered.
#![allow(dead_code)]

use std::{
    collections::{HashMap, HashSet},
    io,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use extrema_infra::{
    arch::market_assets::api_data::{price_data::OrderBookData, utils_data::InstrumentInfo},
    prelude::*,
};
use futures_util::{SinkExt, StreamExt};
use perp_dex::prelude::*;
use serde_json::Value;
use tokio::{net::TcpListener, sync::mpsc, sync::oneshot};
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{ErrorResponse, Request, Response},
    },
};

const QUIET: Duration = Duration::from_millis(1500);
const DEADLINE: Duration = Duration::from_secs(20);

pub struct Fixture {
    pub sub: Value,
    pub frames: Vec<String>,
}

pub fn load_fixture(name: &str) -> Fixture {
    let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let raw: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();

    Fixture {
        sub: raw["sub"].clone(),
        frames: raw["frames"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f.as_str().unwrap().to_string())
            .collect(),
    }
}

pub fn parsed_frames(name: &str) -> Vec<Value> {
    load_fixture(name)
        .frames
        .iter()
        .map(|f| serde_json::from_str(f).unwrap())
        .collect()
}

pub fn count_frames(fixture: &str, keep: impl Fn(&Value) -> bool) -> usize {
    parsed_frames(fixture).iter().filter(|f| keep(f)).count()
}

/// One websocket task. `fixture` is only read by `replay`; an empty `inst`
/// subscribes without instruments, for all-market streams.
pub struct Case {
    pub task_id: u64,
    pub client: PerpDexClients,
    pub channel: WsChannel,
    pub inst: String,
    pub fixture: &'static str,
    pub keepalive: Option<WsKeepalive>,
}

impl Case {
    pub fn new(
        task_id: u64,
        client: PerpDexClients,
        channel: WsChannel,
        inst: &str,
        fixture: &'static str,
    ) -> Self {
        Self {
            task_id,
            client,
            channel,
            inst: inst.to_string(),
            fixture,
            keepalive: None,
        }
    }

    /// Calls `keepalive.on_frame` from the task's `on_lob` / `on_trade`.
    pub fn with_keepalive(mut self, keepalive: WsKeepalive) -> Self {
        self.keepalive = Some(keepalive);
        self
    }
}

pub fn bbo() -> WsChannel {
    WsChannel::Lob(Some(LobParam::Bbo { frequency: None }))
}

#[derive(Clone, Default)]
pub struct LogBuf(Arc<Mutex<Vec<u8>>>);

impl io::Write for LogBuf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl LogBuf {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

pub fn captured_logs() -> LogBuf {
    static LOGS: OnceLock<LogBuf> = OnceLock::new();
    LOGS.get_or_init(|| {
        let logs = LogBuf::default();
        let writer = logs.clone();
        tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::INFO)
            .with_writer(move || writer.clone())
            .init();
        logs
    })
    .clone()
}

async fn serve(
    listener: TcpListener,
    fixtures: Arc<Vec<Fixture>>,
    unmatched: Arc<Mutex<Vec<String>>>,
) {
    loop {
        let (stream, _) = listener.accept().await.unwrap();
        let fixtures = fixtures.clone();
        let unmatched = unmatched.clone();
        tokio::spawn(async move {
            let mut path = String::new();
            #[allow(clippy::result_large_err)]
            let record_path = |req: &Request, res: Response| {
                path = req.uri().to_string();
                Ok::<_, ErrorResponse>(res)
            };
            let mut ws = accept_hdr_async(stream, record_path).await.unwrap();
            let fixture = if path == "/" {
                let Some(Ok(Message::Text(sub))) = ws.next().await else {
                    return;
                };
                let sub_value: Value = serde_json::from_str(&sub).unwrap_or(Value::Null);
                fixtures
                    .iter()
                    .find(|f| f.sub == sub_value)
                    .ok_or_else(|| sub.to_string())
            } else {
                fixtures
                    .iter()
                    .find(|f| f.sub.as_str().is_some_and(|url| url_path(url) == path))
                    .ok_or(path)
            };
            match fixture {
                Ok(fixture) => {
                    for frame in &fixture.frames {
                        ws.send(Message::text(frame.clone())).await.unwrap();
                    }
                },
                Err(sub) => unmatched.lock().unwrap().push(sub),
            }
            while ws.next().await.is_some() {}
        });
    }
}

enum Received {
    Connected(u64),
    Lob(u64, WsLob),
    Trade(u64, WsTrade),
    Other(u64),
}

#[derive(Clone)]
struct Probe {
    mock_url: Option<String>,
    subs: Arc<HashMap<u64, (PerpDexClients, String)>>,
    keepalives: HashMap<u64, (WsChannel, WsKeepalive)>,
    events: mpsc::UnboundedSender<Received>,
    registry: Arc<CommandRegistry>,
}

impl Strategy for Probe {
    async fn initialize(&mut self) {}
}

impl CommandEmitter for Probe {
    fn command_init(&mut self, registry: Arc<CommandRegistry>) {
        self.registry = registry;
    }

    fn command_registry(&self) -> Arc<CommandRegistry> {
        self.registry.clone()
    }
}

impl Probe {
    async fn keepalive(&mut self, task_id: u64) {
        let Some(channel) = self.keepalives.get(&task_id).map(|(c, _)| c.clone()) else {
            return;
        };
        let Some(handle) = self.find_ws_handle(&channel, task_id) else {
            return;
        };
        if let Some((_, keepalive)) = self.keepalives.get_mut(&task_id) {
            let _ = keepalive.on_frame(&handle).await;
        }
    }
}

/// `/stream.extended.exchange/v1/orderbooks/BTC-USD?depth=1` of a websocket URL.
fn url_path(url: &str) -> &str {
    url.split_once("://")
        .and_then(|(_, rest)| rest.find('/').map(|i| &rest[i..]))
        .unwrap_or("/")
}

/// Where to connect and what to send after it; Extended's stream URL is its subscription.
async fn connect_plan(
    client: &PerpDexClients,
    channel: &WsChannel,
    inst: &str,
) -> (WsConnectTarget, Option<String>) {
    let inst = (!inst.is_empty()).then_some(inst);
    if let PerpDexClients::Extended(cli) = client {
        let target = cli
            .get_public_stream_target(channel, inst)
            .expect("stream target");
        return (target, None);
    }

    let url = client.get_public_connect_msg(channel).await.unwrap();
    let insts = inst.map(|inst| vec![inst.to_string()]);
    let sub = client
        .get_public_sub_msg(channel, insts.as_deref())
        .await
        .expect("subscribe message");
    (WsConnectTarget::new(url), Some(sub))
}

impl EventHandler for Probe {
    async fn on_ws_event(&mut self, msg: InfraMsg<WsTaskInfo>) {
        let (client, inst) = &self.subs[&msg.task_id];
        let channel = &msg.data.ws_channel;
        let (mut target, sub) = connect_plan(client, channel, inst).await;
        if let Some(mock) = &self.mock_url {
            target.url = match sub {
                Some(_) => mock.clone(),
                None => format!("{mock}{}", url_path(&target.url)),
            };
        }
        let handle = self
            .find_ws_handle(channel, msg.task_id)
            .expect("websocket task handle");
        let (tx, rx) = oneshot::channel();
        handle
            .send_command(
                TaskCommand::WsConnectWithTarget {
                    target,
                    ack: AckHandle::new(tx),
                },
                Some((AckStatus::WsConnect, rx)),
            )
            .await
            .expect("connect");
        if let Some(sub) = sub {
            handle
                .send_command(
                    TaskCommand::WsMessage {
                        msg: sub,
                        ack: AckHandle::none(),
                    },
                    None,
                )
                .await
                .expect("subscribe");
        }
        let _ = self.events.send(Received::Connected(msg.task_id));
    }

    async fn on_lob(&mut self, msg: InfraMsg<Vec<WsLob>>) {
        self.keepalive(msg.task_id).await;
        for lob in msg.data.iter() {
            let _ = self.events.send(Received::Lob(msg.task_id, lob.clone()));
        }
    }

    async fn on_trade(&mut self, msg: InfraMsg<Vec<WsTrade>>) {
        self.keepalive(msg.task_id).await;
        for trade in msg.data.iter() {
            let _ = self
                .events
                .send(Received::Trade(msg.task_id, trade.clone()));
        }
    }

    async fn on_ws_other(&mut self, msg: InfraMsg<Vec<WsOtherMessage>>) {
        let _ = self.events.send(Received::Other(msg.task_id));
    }
}

#[derive(Default)]
pub struct Run {
    pub connects: HashMap<u64, usize>,
    pub lobs: HashMap<u64, Vec<WsLob>>,
    pub trades: HashMap<u64, Vec<WsTrade>>,
    pub others: HashSet<u64>,
    pub unmatched: Vec<String>,
    pub logs: String,
}

impl Run {
    pub fn lobs(&self, task_id: u64) -> &[WsLob] {
        self.lobs.get(&task_id).map_or(&[], Vec::as_slice)
    }

    pub fn trades(&self, task_id: u64) -> &[WsTrade] {
        self.trades.get(&task_id).map_or(&[], Vec::as_slice)
    }

    pub fn summary(&self) -> String {
        let mut lobs: Vec<_> = self.lobs.iter().map(|(k, v)| (*k, v.len())).collect();
        let mut trades: Vec<_> = self.trades.iter().map(|(k, v)| (*k, v.len())).collect();
        lobs.sort();
        trades.sort();
        format!(
            "lob events {lobs:?}, trade events {trades:?}, connects {:?}, unmatched {:?}",
            self.connects, self.unmatched
        )
    }
}

enum Stop {
    Quiet,
    After(Duration),
}

async fn run(cases: Vec<Case>, mock_url: Option<String>, stop: Stop) -> Run {
    let logs = captured_logs();
    let (events, mut received) = mpsc::unbounded_channel();
    let probe = Probe {
        mock_url,
        subs: Arc::new(
            cases
                .iter()
                .map(|c| (c.task_id, (c.client.clone(), c.inst.clone())))
                .collect(),
        ),
        keepalives: cases
            .iter()
            .filter_map(|c| {
                c.keepalive
                    .clone()
                    .map(|k| (c.task_id, (c.channel.clone(), k)))
            })
            .collect(),
        events,
        registry: Arc::new(CommandRegistry::default()),
    };

    let mut builder = EnvBuilder::new()
        .with_ws_decoder(LighterWs)
        .with_ws_decoder(LighterRhWs)
        .with_ws_decoder(AsterWs)
        .with_ws_decoder(ArcusWs)
        .with_ws_decoder(GrvtWs)
        .with_ws_decoder(PacificaWs)
        .with_ws_decoder(EdgexWs)
        .with_ws_decoder(NadoWs)
        .with_ws_decoder(ApexWs)
        .with_ws_decoder(ExtendedWs);
    for case in &cases {
        builder = builder.with_task(WsTaskInfo {
            market: case.client.market(),
            ws_channel: case.channel.clone(),
            filter_channels: false,
            chunk: 1,
            task_base_id: Some(case.task_id),
        });
    }
    let runtime = tokio::spawn(
        builder
            .with_strategy_module(probe)
            .build()
            .unwrap()
            .execute(),
    );

    let mut out = Run::default();
    let started = tokio::time::Instant::now();
    let mut idle = DEADLINE;
    loop {
        let wait = match stop {
            Stop::Quiet => idle,
            Stop::After(total) => total.saturating_sub(started.elapsed()),
        };
        match tokio::time::timeout(wait, received.recv()).await {
            Ok(Some(Received::Connected(task))) => *out.connects.entry(task).or_default() += 1,
            Ok(Some(Received::Lob(task, lob))) => {
                out.lobs.entry(task).or_default().push(lob);
                idle = QUIET;
            },
            Ok(Some(Received::Trade(task, trade))) => {
                out.trades.entry(task).or_default().push(trade);
                idle = QUIET;
            },
            Ok(Some(Received::Other(task))) => {
                out.others.insert(task);
                idle = QUIET;
            },
            Ok(None) | Err(_) => break,
        }
        if matches!(stop, Stop::Quiet) {
            assert!(started.elapsed() < DEADLINE, "replay did not settle");
        }
    }

    runtime.abort();
    out.logs = logs.text();
    out
}

/// Replays each case's fixture through the runtime until the events settle.
pub async fn replay(cases: Vec<Case>) -> Run {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let fixtures = Arc::new(cases.iter().map(|c| load_fixture(c.fixture)).collect());
    let unmatched = Arc::new(Mutex::new(Vec::new()));
    let server = tokio::spawn(serve(listener, fixtures, unmatched.clone()));

    let mut out = run(cases, Some(url), Stop::Quiet).await;
    server.abort();
    out.unmatched = unmatched.lock().unwrap().clone();
    out
}

/// Streams every case from its live venue for `duration`.
pub async fn live(cases: Vec<Case>, duration: Duration) -> Run {
    run(cases, None, Stop::After(duration)).await
}

pub fn assert_clean(run: &Run) {
    assert!(
        run.unmatched.is_empty(),
        "subscribe messages differ from live: {:?}",
        run.unmatched
    );
    for noise in [
        "Failed to deserialize",
        "WS error",
        "subscription error",
        "degraded",
    ] {
        assert!(!run.logs.contains(noise), "{noise}:\n{}", run.logs);
    }
}

pub fn assert_book_side_order(lob: &WsLob) {
    assert!(
        lob.bids.windows(2).all(|w| w[0].price > w[1].price),
        "{lob:?}"
    );
    assert!(
        lob.asks.windows(2).all(|w| w[0].price < w[1].price),
        "{lob:?}"
    );
    if let (Some(bid), Some(ask)) = (lob.bids.first(), lob.asks.first()) {
        assert!(bid.price < ask.price, "{lob:?}");
    }
}

pub fn assert_bbo(lob: &WsLob) {
    assert!(matches!(lob.event, LobEventKind::Bbo), "{lob:?}");
    assert_eq!((lob.bids.len(), lob.asks.len()), (1, 1), "{lob:?}");
    assert!(
        0.0 < lob.bids[0].price && lob.bids[0].price < lob.asks[0].price,
        "{lob:?}"
    );
}

/// Each event's `seq.prev` equals the previous event's `seq.last`.
pub fn assert_prev_chain(lobs: &[WsLob]) {
    for pair in lobs.windows(2) {
        assert_eq!(
            pair[1].seq.as_ref().unwrap().prev,
            pair[0].seq.as_ref().unwrap().last,
            "sequence gap"
        );
    }
}

/// Each event's `seq.last` is the previous one + 1.
pub fn assert_last_contiguous(lobs: &[WsLob]) {
    for pair in lobs.windows(2) {
        let prev = pair[0].seq.as_ref().unwrap().last.unwrap();
        let next = pair[1].seq.as_ref().unwrap().last.unwrap();
        assert_eq!(next, prev + 1, "sequence gap");
    }
}

/// Each event's `seq.last` is above the previous one, or at least equal unless `strictly`.
pub fn assert_last_rising(lobs: &[WsLob], strictly: bool) {
    for pair in lobs.windows(2) {
        let prev = pair[0].seq.as_ref().unwrap().last.unwrap();
        let next = pair[1].seq.as_ref().unwrap().last.unwrap();
        assert!(
            next > prev || (!strictly && next == prev),
            "sequence went back: {prev} -> {next}"
        );
    }
}

pub fn assert_sane_book(book: &OrderBookData, inst: &str, max_levels: usize) {
    assert_eq!(book.inst, inst);
    assert!(!book.bids.is_empty() && !book.asks.is_empty(), "{book:?}");
    assert!(book.bids.len() <= max_levels && book.asks.len() <= max_levels);
    assert!(
        book.bids.windows(2).all(|w| w[0].0 > w[1].0),
        "bids not descending"
    );
    assert!(
        book.asks.windows(2).all(|w| w[0].0 < w[1].0),
        "asks not ascending"
    );
    assert!(book.bids[0].0 < book.asks[0].0, "crossed book");
    assert!(
        book.bids
            .iter()
            .chain(&book.asks)
            .all(|(p, s)| *p > 0.0 && *s > 0.0)
    );
    assert!(
        book.timestamp > 1_700_000_000_000_000,
        "timestamp is not in micros"
    );
}

pub fn assert_sane_instruments(infos: &[InstrumentInfo]) {
    assert!(!infos.is_empty());
    assert_eq!(
        infos.iter().map(|i| &i.inst).collect::<HashSet<_>>().len(),
        infos.len(),
        "duplicate instruments"
    );
    for info in infos {
        assert!(info.tick_size > 0.0 && info.lot_size > 0.0, "{info:?}");
        assert!(
            info.min_lmt_size > 0.0 && info.max_lmt_size >= info.min_lmt_size,
            "{info:?}"
        );
    }
}

pub fn mid(lob: &WsLob) -> f64 {
    (lob.bids[0].price + lob.asks[0].price) / 2.0
}
