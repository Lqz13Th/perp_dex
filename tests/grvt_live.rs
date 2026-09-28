//! GRVT against the live venue: `cargo test --test grvt_live -- --ignored`.

mod common;

use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};

use common::*;
use extrema_infra::{
    arch::market_assets::{
        api_data::{price_data::OrderBookData, utils_data::InstrumentInfo},
        api_general::get_micros_timestamp,
    },
    prelude::*,
};
use perp_dex::prelude::*;
use reqwest::Client;

const RUN_FOR: Duration = Duration::from_secs(30);
const MINUTE_US: u64 = 60_000_000;

fn shared_client() -> Arc<Client> {
    Arc::new(Client::new())
}

fn assert_recent(timestamp_us: u64) {
    let now = get_micros_timestamp();
    assert!(
        timestamp_us + 10 * MINUTE_US > now && timestamp_us < now + MINUTE_US,
        "{timestamp_us} is not a recent timestamp in micros"
    );
}

fn assert_sane_book(book: &OrderBookData, inst: &str, max_levels: usize) {
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
    assert_recent(book.timestamp);
}

fn assert_sane_instruments(infos: &[InstrumentInfo]) {
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
        assert!(info.min_notional.is_some() && info.inst_code.is_some());
    }
}

#[tokio::test]
#[ignore = "hits the live GRVT API"]
async fn grvt_public_rest() {
    let cli = GrvtCli::new(shared_client());

    let instruments = cli.get_all_instruments().await.unwrap();
    for i in &instruments {
        assert_eq!(
            i.inst(),
            format!("{}_{}_PERP", i.base, i.quote),
            "name parsing disagrees with all_instruments for {}",
            i.instrument
        );
        assert_eq!(cli_perp_to_grvt_inst(&i.inst()).unwrap(), i.instrument);
        assert_eq!(i.kind, "PERPETUAL");
    }
    for stock in ["NVDA", "TSLA", "AAPL", "SPY"] {
        assert!(
            instruments
                .iter()
                .any(|i| i.base == stock && i.status.as_deref() == Some("ACTIVE")),
            "GRVT lists {stock}"
        );
    }
    let active = instruments
        .iter()
        .filter(|i| i.status.as_deref() == Some("ACTIVE"))
        .count();

    let perps = cli
        .get_instrument_info(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert_sane_instruments(&perps);
    assert_eq!(perps.len(), instruments.len());
    assert!(
        cli.get_instrument_info(InstrumentType::Spot)
            .await
            .unwrap()
            .is_empty()
    );
    let nvda = perps.iter().find(|i| i.inst == "NVDA_USDT_PERP").unwrap();
    assert_eq!(nvda.state, InstrumentStatus::Live);
    assert_eq!(nvda.tick_size, 0.01);

    let live = cli
        .get_live_instruments(InstrumentType::Perpetual)
        .await
        .unwrap();
    let perp_insts: HashSet<_> = perps.iter().map(|i| i.inst.clone()).collect();
    assert_eq!(live.len(), active);
    assert!(live.iter().all(|i| perp_insts.contains(i)));
    assert!(live.contains(&"BTC_USDT_PERP".to_string()));

    let mini = cli.get_mini_ticker("NVDA_USDT_PERP").await.unwrap();
    assert_eq!(mini.instrument, "NVDA_USDT_Perp");
    let bid: f64 = mini.best_bid_price.as_deref().unwrap().parse().unwrap();
    let ask: f64 = mini.best_ask_price.as_deref().unwrap().parse().unwrap();
    assert!(0.0 < bid && bid < ask, "{mini:?}");
    let err = cli.get_mini_ticker("NOPE_USDT_PERP").await.unwrap_err();
    assert!(err.to_string().contains("GRVT REST error"), "{err}");
    assert!(matches!(
        cli.get_mini_ticker("NVDA-USDT").await,
        Err(InfraError::ApiCliError(_))
    ));

    let insts = vec!["NVDA_USDT_PERP".to_string(), "BTC_USDT_PERP".to_string()];
    let tickers = cli.get_tickers(Some(&insts), None).await.unwrap();
    assert_eq!(
        tickers.iter().map(|t| t.inst.clone()).collect::<Vec<_>>(),
        insts
    );
    for ticker in &tickers {
        assert!(ticker.price > 0.0 && ticker.inst_type == InstrumentType::Perpetual);
        assert_recent(ticker.timestamp);
    }
    let marks = cli
        .get_mark_prices(Some(&insts), Some(InstrumentType::Perpetual))
        .await
        .unwrap();
    assert_eq!(marks.len(), 2);
    for mark in &marks {
        let last = tickers.iter().find(|t| t.inst == mark.inst).unwrap().price;
        assert!(
            (mark.mark_price / last - 1.0).abs() < 0.05,
            "{mark:?} vs {last}"
        );
        assert_recent(mark.timestamp);
    }
    let all_tickers = cli.get_tickers(None, None).await.unwrap();
    assert_eq!(
        all_tickers
            .iter()
            .map(|t| t.inst.clone())
            .collect::<HashSet<_>>(),
        live.iter().cloned().collect()
    );
    assert_eq!(
        cli.get_mark_prices(None, Some(InstrumentType::Perpetual))
            .await
            .unwrap()
            .len(),
        live.len()
    );
    assert!(
        cli.get_tickers(None, Some(InstrumentType::Spot))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        cli.get_tickers(
            Some(&["NVDA_USDT_PERP".to_string(), "NOPE_USDT_PERP".to_string()]),
            None
        )
        .await
        .is_err()
    );

    let book = cli
        .get_orderbook("NVDA_USDT_PERP", InstrumentType::Perpetual, 10)
        .await
        .unwrap();
    assert_sane_book(&book, "NVDA_USDT_PERP", 10);
    let fifty = cli
        .get_orderbook("BTC_USDT_PERP", InstrumentType::Perpetual, 50)
        .await
        .unwrap();
    assert_sane_book(&fifty, "BTC_USDT_PERP", 50);
    let deep = cli
        .get_orderbook("BTC_USDT_PERP", InstrumentType::Perpetual, 0)
        .await
        .unwrap();
    assert_sane_book(&deep, "BTC_USDT_PERP", 500);
    assert!(deep.bids.len() > 50);
    for (inst, inst_type, depth) in [
        ("NVDA_USDT_PERP", InstrumentType::Perpetual, 5),
        ("NVDA_USDT_PERP", InstrumentType::Spot, 10),
        ("NVDA-USDT", InstrumentType::Perpetual, 10),
    ] {
        assert!(matches!(
            cli.get_orderbook(inst, inst_type, depth).await,
            Err(InfraError::ApiCliError(_))
        ));
    }
    let err = cli
        .get_orderbook("NOPE_USDT_PERP", InstrumentType::Perpetual, 10)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("GRVT REST error"), "{err}");
}

#[tokio::test]
#[ignore = "hits the live GRVT API"]
async fn perp_dex_clients_dispatch_to_grvt() {
    let venue = PerpDexClients::Grvt(GrvtCli::new(shared_client()));
    let inst = "NVDA_USDT_PERP";

    assert_eq!(venue.market(), GRVT);
    let infos = venue
        .get_instrument_info(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert!(infos.iter().any(|i| i.inst == inst));
    assert!(
        venue
            .get_live_instruments(InstrumentType::Perpetual)
            .await
            .unwrap()
            .contains(&inst.to_string())
    );

    let book = venue
        .get_orderbook(inst, InstrumentType::Perpetual, 10)
        .await
        .unwrap();
    assert_sane_book(&book, inst, 10);
    let marks = venue
        .get_mark_prices(Some(&[inst.to_string()]), Some(InstrumentType::Perpetual))
        .await
        .unwrap();
    let mid = (book.bids[0].0 + book.asks[0].0) / 2.0;
    assert_eq!(marks.len(), 1);
    assert!((marks[0].mark_price / mid - 1.0).abs() < 0.05);
    assert_eq!(
        venue
            .get_tickers(Some(&[inst.to_string()]), None)
            .await
            .unwrap()
            .len(),
        1
    );

    let channel = bbo();
    assert_eq!(
        venue.get_public_connect_msg(&channel).await.unwrap(),
        "wss://market-data.grvt.io/ws/full"
    );
    assert!(
        venue
            .get_public_sub_msg(&channel, Some(&[inst.to_string()]))
            .await
            .unwrap()
            .contains("NVDA_USDT_Perp@200")
    );
}

/// Applies a snapshot and its deltas; the result must stay uncrossed.
fn rebuild_book(lobs: &[WsLob]) -> (f64, f64) {
    let key = |price: f64| (price * 1e9).round() as i64;
    let mut bids = BTreeMap::new();
    let mut asks = BTreeMap::new();

    for lob in lobs {
        if matches!(lob.event, LobEventKind::Snapshot) {
            bids.clear();
            asks.clear();
        }
        for (side, levels) in [(&mut bids, &lob.bids), (&mut asks, &lob.asks)] {
            for level in levels {
                match level.action {
                    LobLevelAction::Delete => side.remove(&key(level.price)),
                    LobLevelAction::Upsert => side.insert(key(level.price), level.price),
                };
            }
        }
    }

    let bid = *bids.values().next_back().unwrap();
    let ask = *asks.values().next().unwrap();
    assert!(bid < ask, "rebuilt book is crossed: {bid} >= {ask}");
    (bid, ask)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "streams from live GRVT and Aster for 30 seconds"]
async fn grvt_live_streams_decode_cleanly() {
    let grvt = PerpDexClients::Grvt(GrvtCli::default());
    let aster = PerpDexClients::Aster(AsterCli::default());
    let snapshot = WsChannel::Lob(Some(LobParam::Snapshot {
        depth: Some(10),
        frequency: None,
    }));
    let mut cases = Vec::new();
    for (base, inst) in [(0, "BTC_USDT_PERP"), (10, "NVDA_USDT_PERP")] {
        cases.push(Case::new(
            base + 1,
            grvt.clone(),
            WsChannel::Lob(None),
            inst,
            "",
        ));
        cases.push(Case::new(
            base + 2,
            grvt.clone(),
            snapshot.clone(),
            inst,
            "",
        ));
        cases.push(Case::new(base + 3, grvt.clone(), bbo(), inst, ""));
        cases.push(Case::new(
            base + 4,
            grvt.clone(),
            WsChannel::Trades(None),
            inst,
            "",
        ));
    }
    cases.push(Case::new(
        5,
        grvt.clone(),
        WsChannel::Lob(Some(LobParam::Incremental {
            depth: None,
            frequency: Some(LobFrequency::Ms100),
        })),
        "BTC_USDT_PERP",
        "",
    ));
    cases.push(Case::new(
        6,
        grvt,
        WsChannel::Other("v1.mini.d@0".into()),
        "BTC_USDT_PERP",
        "",
    ));
    cases.push(Case::new(21, aster, bbo(), "NVDA_USDT_PERP", ""));

    let started = get_micros_timestamp();
    let run = live(cases, RUN_FOR).await;
    eprintln!("{}", run.summary());

    assert_clean(&run);
    assert!(
        run.connects.values().all(|n| *n == 1),
        "reconnected: {:?}",
        run.connects
    );
    assert!(run.others.contains(&6), "no raw frames on task 6");

    for (base, inst) in [(0, "BTC_USDT_PERP"), (10, "NVDA_USDT_PERP")] {
        for id in 1..=3 {
            let lobs = run.lobs(base + id);
            assert!(!lobs.is_empty(), "no book events on task {}", base + id);
            for lob in lobs {
                assert!(lob.market == GRVT && lob.inst == inst, "{lob:?}");
                assert_recent(lob.timestamp);
            }
        }

        let book = run.lobs(base + 1);
        assert!(matches!(book[0].event, LobEventKind::Snapshot));
        assert_book_side_order(&book[0]);
        assert!(
            book[1..].iter().all(|l| {
                matches!(l.event, LobEventKind::Incremental | LobEventKind::Heartbeat)
            })
        );
        if book.len() > 2 {
            assert_prev_chain(&book[1..]);
            assert_last_contiguous(&book[1..]);
        }
        let (bid, ask) = rebuild_book(book);

        let snaps = run.lobs(base + 2);
        for lob in snaps {
            assert!(matches!(lob.event, LobEventKind::Snapshot));
            assert!(lob.bids.len() <= 10 && lob.asks.len() <= 10);
            assert_book_side_order(lob);
        }
        if snaps.len() > 2 {
            assert_prev_chain(&snaps[1..]);
        }

        let quotes = run.lobs(base + 3);
        quotes.iter().for_each(assert_bbo);
        if quotes.len() > 2 {
            assert_prev_chain(&quotes[1..]);
        }
        let quote_mid = mid(quotes.last().unwrap());
        assert!(
            ((bid + ask) / 2.0 / quote_mid - 1.0).abs() < 0.005,
            "{inst}: rebuilt book {bid}/{ask} far from BBO mid {quote_mid}"
        );

        let trades = run.trades(base + 4);
        for trade in trades {
            assert!(trade.market == GRVT && trade.inst == inst);
            assert!(trade.price > 0.0 && trade.size > 0.0);
            assert!(
                trade.timestamp + MINUTE_US > started,
                "replayed fill: {trade:?}"
            );
        }
        assert_eq!(
            trades
                .iter()
                .map(|t| t.trade_id)
                .collect::<HashSet<_>>()
                .len(),
            trades.len()
        );
    }

    let book_100ms = run.lobs(5);
    assert!(matches!(book_100ms[0].event, LobEventKind::Snapshot));
    if book_100ms.len() > 2 {
        assert_last_contiguous(&book_100ms[1..]);
    }
    rebuild_book(book_100ms);

    let grvt_mid = mid(run.lobs(13).last().unwrap());
    let aster_mid = mid(run.lobs(21).last().unwrap());
    eprintln!("NVDA mids (GRVT, Aster): {grvt_mid}, {aster_mid}");
    assert!(
        (grvt_mid / aster_mid - 1.0).abs() < 0.02,
        "NVDA mids disagree: {grvt_mid} vs {aster_mid}"
    );
}
