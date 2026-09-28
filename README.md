# Perp DEX

Stock-perp DEX venues for [`extrema_infra`](https://github.com/Lqz13Th/extrema_infra).

- Each venue plugs into infra as an external venue: a `LobWsDecoder` on `Market::Custom(id)` and a client implementing `LobPublicRest` and `LobWebsocket`, laid out like infra's native exchanges.

- `PerpDexClients` dispatches over every client and infra's `HyperliquidCli`, so one enum covers Hyperliquid builder DEXes (xyz, ...) and the venues here.

Public market data only; no signing or order entry yet.

---

## Venues

| venue | `Market` | decoder | client | instrument |
|---|---|---|---|---|
| Lighter | `Custom(1)` | `LighterWs` | `LighterCli` | `@110` (market id) |
| Lighter on Robinhood Chain | `Custom(4)` | `LighterRhWs` | `LighterCli` + `set_venue(LighterVenue::Robinhood)` | `@15` (its own ids) |
| Aster | `Custom(2)` | `AsterWs` | `AsterCli` | `NVDA_USDT_PERP` |
| Arcus | `Custom(3)` | `ArcusWs` | `ArcusCli` | `NVDA_USD_PERP` |
| GRVT | `Custom(5)` | `GrvtWs` | `GrvtCli` | `NVDA_USDT_PERP` |
| Extended | `Custom(6)` | `ExtendedWs` | `ExtendedCli` | `NVDA_24_5_USD_PERP` |
| edgeX | `Custom(7)` | `EdgexWs` | `EdgexCli` | `@30000020` (contract id) |
| ApeX Omni | `Custom(8)` | `ApexWs` | `ApexCli` | `NVDA_USDT_PERP` |
| Nado | `Custom(9)` | `NadoWs` | `NadoCli` | `@112` (product id) |
| Pacifica | `Custom(10)` | `PacificaWs` | `PacificaCli` | `NVDA_USDC_PERP` |

- Venues whose frames carry only a numeric id use `@<id>`; the venue symbol is in `InstrumentInfo::inst_code`.
- Each client also returns its raw market list (`get_exchange_info`, `get_markets`, `get_symbols`, ...), with `is_stock()` / `is_equity()` where the venue classifies stocks.
- Every venue is a feature; `all`, the default, enables them all.

---

## Usage

```toml
[dependencies]
extrema_infra = { version = "0.5.2", features = ["hyperliquid"] }
perp_dex = "0.1"
```

Register a decoder per venue and declare tasks on its `Market`:

```rust
use extrema_infra::prelude::*;
use perp_dex::prelude::*;

let env = EnvBuilder::new()
    .with_ws_decoder(LighterWs)
    .with_ws_decoder(AsterWs)
    .with_task(WsTaskInfo {
        market: LIGHTER,
        ws_channel: WsChannel::Lob(None),
        filter_channels: false,
        chunk: 1,
        task_base_id: Some(1),
    })
    .with_strategy_module(strategy)
    .build()?;
```

On every (re)connect, the strategy's `on_ws_event` connects and subscribes through the client:

```rust
let url = cli.get_public_connect_msg(channel).await?;
handle
    .send_command(TaskCommand::WsConnect { msg: url, ack: AckHandle::new(tx) }, Some((AckStatus::WsConnect, rx)))
    .await?;

let sub = cli.get_public_sub_msg(channel, Some(&[inst.to_string()])).await?;
handle
    .send_command(TaskCommand::WsMessage { msg: sub, ack: AckHandle::none() }, None)
    .await?;
```

`examples/stock_bbo.rs` puts one stock's BBO from every venue with a per-market BBO stream on one `on_lob`, with each venue's deviation from the median mid:

```bash
cargo run --example stock_bbo -- NVDA 60
```

---

## Streams

| venue | `Lob(Bbo)` | `Lob(Snapshot)` | `Lob(None / Incremental)` | `Trades` |
|---|---|---|---|---|
| Lighter | `ticker` | - | `order_book` | `trade` |
| Aster | `bookTicker` | `depth{5,10,20}` | `depth` diffs | `trade` / `aggTrade` (default) |
| Arcus | `bbo` | `l2Orderbook` (~200 ms) | `l2OrderbookUpdates` | `trades` |
| GRVT | `v1.mini.s` (200 / 500 / 1000 ms) | `v1.book.s` (10 / 50 / 100 / 500 levels) | `v1.book.d` (50 / 100 / 500 / 1000 ms) | `v1.trade` |
| Extended | `orderbooks/{m}?depth=1` | - | `orderbooks/{m}` | `publicTrades/{m}` |
| edgeX | `bookTicker.all.1s`, no instruments | - | `depth.{id}.{15,200}` | `trades.{id}` |
| ApeX | - | - | `orderBook{200,25}.H` | `recentlyTrade.H` |
| Nado | `best_bid_offer` | - | `book_depth` (~50 ms) | `trade` |
| Pacifica | `bbo` | `book` (10 levels, 250 ms) | `book`, snapshots | `trades` |

- `Other(channel)` delivers raw frames of that channel on every venue.
- Unsupported parameters are an `ApiCliError`.
- Subscribe replies that replay trade history are not emitted as trades.

---

## Book Continuity

Keeping a local book consistent is the strategy's job, as with infra's venues. Events carry `seq` for it:

- **Lighter, edgeX, GRVT:** a snapshot on subscribe; each delta's `seq.prev` must equal the previous `seq.last`. GRVT's snapshot has no sequence and restarts with its gateway.
- **Aster:** diffs need a REST snapshot spliced in (`U <= lastUpdateId <= u`, then `pu` chains).
- **Arcus:** a snapshot on subscribe; after the first delta, `seq.last` rises by exactly 1.
- **ApeX:** a snapshot on subscribe; each delta's `seq.last` is the previous one + 1.
- **Extended:** the stream opens with the latest minutely snapshot and replays the deltas since; `seq.last` counts every frame of the connection from 1, so any gap means reconnect.
- **Nado:** no snapshot; seed from `NadoCli::get_market_liquidity`, apply diffs whose `seq.last` (ns) is above its `timestamp`, then each `seq.prev` must equal the previous `seq.last`.
- **Pacifica:** every `book` frame is a full snapshot; `seq.last` never decreases.

A `timestamp` of 0 means the venue sent no exchange time (Arcus and edgeX deltas). Nado trades carry no id, so their `trade_id` is 0; GRVT, edgeX and ApeX trade ids are not plain numbers and are packed into `u64` as documented on each decoder.

---

## Keepalive

Infra only pings after ten silent seconds, but Lighter (2 min), Pacifica (60 s) and ApeX (150 s after the last `pong`) drop a connection without a recent client frame, however busy it is.

Keep a `WsKeepalive` per such task, from `lighter_keepalive()`, `pacifica_keepalive()` or `apex_keepalive()`, and call it from the task's `on_lob` / `on_trade`:

```rust
if let Some(handle) = self.find_ws_handle(&channel, msg.task_id) {
    keepalive.on_frame(&handle).await?;
}
```

It sends at most every 30 s and only while frames arrive, so no frame is queued ahead of infra's reconnect `WsConnect`.

---

## Extended

Every Extended stream is its own URL with no subscribe message, and the venue refuses the upgrade without a `User-Agent`. Connect an Extended task with `ExtendedCli::get_public_stream_target(channel, Some(inst))` and `TaskCommand::WsConnectWithTarget`, and send nothing after it; its `LobWebsocket` methods return an `ApiCliError` saying so.

---

## REST Notes

- GRVT and edgeX have no bulk ticker: `get_tickers` / `get_mark_prices` with `insts = None` make one request per instrument.
- Nado's Cloudflare websocket hosts require `permessage-deflate`, so `NadoCli` connects to the documented direct gateway.

---

## Tests

- `cargo test`: unit tests plus the `*_replay` tests, which replay frames captured from the live venues through the infra runtime.
- `cargo test --tests -- --ignored`: every public REST method and 30 s of streams against the live venues, plus the Pacifica and ApeX keepalive runs.

---

## License

This project is licensed under the [Apache 2.0 license](LICENSE).
