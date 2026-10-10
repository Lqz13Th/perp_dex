# Perp DEX

Stock-perp DEX venues for [`extrema_infra`](https://github.com/Lqz13Th/extrema_infra) 0.6.

- Each venue plugs into infra as an external venue: a `LobWsDecoder` on `Market::Custom(id)` and a client implementing `LobPublicRest` and `LobWebsocket`, laid out like infra's native exchanges.
- `PerpDexClients` dispatches over every client and infra's `HyperliquidCli`, so one enum covers Hyperliquid builder DEXes (xyz, EntropyIO's io, Kinetiq's mkts, ...) and the venues here.
- Every venue has public market data: REST tickers, mark prices, books and instruments, plus book and trade streams (and BBO where the venue has one, see [Streams](#streams)). Lighter also has the private REST and websocket API (see [Lighter Private API](#lighter-private-api)).

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
- `LIGHTER`, `LIGHTER_RH`, `ASTER`, ... are the `Market` constants for each row.

---

## Install

```toml
[dependencies]
extrema_infra = { version = "0.6.0", features = ["hyperliquid"] }
perp_dex = "0.2"
```

Every venue is a feature (`lighter`, `aster`, `arcus`, `grvt`, `extended`, `edgex`, `apex`, `nado`, `pacifica`); `all`, the default, enables them all. To build only some venues:

```toml
perp_dex = { version = "0.2", default-features = false, features = ["lighter"] }
```

`lighter` also pulls in the pure-Rust signer (`goldilocks-crypto`, `poseidon-hash`) for the private API.

---

## Usage

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

For a Hyperliquid builder DEX, use infra's `HyperliquidCli` (or `PerpDexClients::Hyperliquid`) with `set_perp_dex(Some(dex))`, then call `init_inst_index_map()` before any perp REST call or order.

---

## Examples

| example | what it does | run |
|---|---|---|
| `stock_bbo` | One stock's BBO from every venue with a per-market BBO stream on one `on_lob`, with each venue's deviation from the median mid | `cargo run --example stock_bbo -- NVDA 60` |
| `bbo_recorder` | Records every BBO update and trade of a set of stocks from every venue with a BBO stream to `bbo-<start>.csv` / `trades-<start>.csv` | `cargo run --release --example bbo_recorder -- NVDA,TSLA 600 /tmp/perp_dex_bbo` |
| `lighter_private` | Checks the key; reads balance, positions, history and fills; places post-only bids 5% below the touch, moves one and cancels them, singly and in batches | `cargo run --example lighter_private --features lighter -- @139 orders` |
| `lighter_private_ws` | Streams orders, positions and fills, and places, moves and cancels a post-only bid over the websocket | `cargo run --example lighter_private_ws --features lighter -- @139` |

The Lighter examples need the credentials of [Lighter Private API](#lighter-private-api) and trade real orders.

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

- Lighter on Robinhood Chain has the same channels as Lighter.
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

Infra only pings after ten silent seconds, but Lighter (2 min), Pacifica (60 s), ApeX (150 s after the last `pong`) and edgeX (about 60 s after the last `pong`) drop a connection without a recent client frame, however busy it is.

Keep a `WsKeepalive` per such task, from `lighter_keepalive()`, `pacifica_keepalive()`, `apex_keepalive()` or `edgex_keepalive()`, and call it from the task's `on_lob` / `on_trade`:

```rust
if let Some(handle) = self.find_ws_handle(&channel, msg.task_id) {
    keepalive.on_frame(&handle).await?;
}
```

It sends at most once per interval (30 s; 20 s for edgeX) and only while frames arrive, so no frame is queued ahead of infra's reconnect `WsConnect`.

---

## Extended

Every Extended stream is its own URL with no subscribe message, and the venue refuses the upgrade without a `User-Agent`. Connect an Extended task with `ExtendedCli::get_public_stream_target(channel, Some(inst))` and `TaskCommand::WsConnectWithTarget`, and send nothing after it; its `LobWebsocket` methods return an `ApiCliError` saying so.

---

## Lighter Private API

`LighterCli` implements infra's `LobPrivateRest` once it has credentials and market scales:

- Credentials: `init_api_key()`, which reads `LIGHTER_ACCOUNT_INDEX`, `LIGHTER_API_KEY_INDEX` and `LIGHTER_API_PRIVATE_KEY` from the environment, or `set_auth(LighterAuth::new(..))`.
- Market scales: `init_market_scales()`, the size and price decimals of every market, loaded once like infra's `init_inst_index_map`.

| | Hyperliquid (infra) | Lighter |
|---|---|---|
| Identity | owner address | account index plus API key index (0–254) |
| Trading key | secp256k1 agent key, 32 bytes | API key, 40 bytes (80 hex), on the ECgFp5 curve |
| Signature | EIP-712 ECDSA | Schnorr over ECgFp5, Poseidon2 hash (Goldilocks) |
| Nonce | ms timestamp | per API key, strictly +1 |
| Amounts | decimal strings | integers scaled by the market's size and price decimals |
| Private reads | no auth | signed auth token in the `authorization` header |

**Signing.** Signing is pure Rust, built on `goldilocks-crypto` and `poseidon-hash`, ports of lighter-go pinned to exact versions. `tests/fixtures/lighter_signer_vectors.json` was generated with the official Go signer (its source is next to it). Against those vectors the tests check:

- every transaction hash and `tx_info` byte for byte;
- the public key derivation;
- the auth token hash;
- the official signatures, which our verifier accepts.

Schnorr nonces are random, so the signatures themselves differ from run to run. Our signatures pass the official verifier.

**Orders.**

- `OrderParams.size` and `price` are decimal strings that must fit the market's decimals exactly.
- Every order carries a price; for `Market` it is the worst acceptable price.
- `PostOnly` and `Limit` orders rest for 28 days. `Ioc` and `Market` orders carry no expiry.
- Client order ids are integers below 2^48.

**Acks.** `sendTx` only reports that the sequencer accepted the transaction. Acks are `Live` (or `Canceled` for cancels) with the transaction hash in `msg`. The exchange `order_index` and fills come from the `AccountOrders` stream or `get_open_orders`.

**History.** `get_order_history` pages through the filled, canceled and expired orders of a market, newest first. `get_orders_by_client_ids` looks orders up by client id. `get_fills` and `get_position_funding` return the latest fills and funding payments of the account, and `get_account_limits` its tier and fee ticks.

**Lighter-only calls.** `modify_order`, `update_leverage`, `update_margin`, `cancel_all_orders`, `next_nonce`, `auth_token`, `check_api_key`.

**Nonces.** The nonce is read once from the exchange and counted locally in an atomic shared by clones. It is re-read after any failed send. Auth tokens are signed per call; there are no locks.

**Websocket.** `get_private_connect_msg` / `get_private_sub_msg` subscribe with a fresh auth token; the exchange checks it only when subscribing.

| task channel | Lighter channel | events |
|---|---|---|
| `AccountOrders` | `account_all_orders` | `on_acc_order`: every order a transaction changed; the subscribe reply lists the open orders |
| `AccountPositions` | `account_all_positions` | `on_acc_pos`, flat positions included |
| `Other(LIGHTER_WS_ACCOUNT_TRADES)` | `account_all_trades` | `on_ws_other`; `WsAccountTradesLighter::into_fills` |
| `Other("user_stats")`, any `account_*` name | that channel | `on_ws_other`, raw |
| `Other(LIGHTER_TX_CHANNEL)` | none | `on_ws_other`; replies parse as `WsSendTxLighter` |

On the `LIGHTER_TX_CHANNEL` connection, transactions go out as `jsonapi/sendtx` frames:

```rust
let txs = cli.sign_orders(&orders).await?; // or sign_cancels / sign_modify
handle
    .send_command(TaskCommand::WsMessage { msg: lighter_ws_send_tx_msg("place-1", &txs[0]), ack: AckHandle::none() }, None)
    .await?;
```

A rejected (or lost) transaction leaves a gap that blocks later nonces, so call `invalidate_nonce()`. The next signing then re-reads the nonce. A batch holds at most 15 transactions.

See [Examples](#examples) for `lighter_private` (REST) and `lighter_private_ws` (websocket).

---

## REST Notes

- GRVT and edgeX have no bulk ticker: `get_tickers` / `get_mark_prices` with `insts = None` make one request per instrument.
- Nado's Cloudflare websocket hosts require `permessage-deflate`, so `NadoCli` connects to the documented direct gateway.

---

## Tests

- `cargo test`: unit tests plus the `*_replay` tests, which replay frames captured from the live venues through the infra runtime.
- `cargo test --tests -- --ignored`: every public REST method and 30 s of streams against the live venues, plus the Pacifica and ApeX keepalive runs.
- `LIGHTER_DUMP_SIGS=<file> cargo test --lib dump_signatures -- --ignored`: writes signatures made by the Rust signer; check them with the official verifier through `go run ./cmd/golden verify < <file>` (see `tests/fixtures/lighter_signer_vectors.go`).

---

## License

This project is licensed under the [Apache 2.0 license](LICENSE).
