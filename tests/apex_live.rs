//! ApeX against the live venue: `cargo test --test apex_live -- --ignored`.

mod common;

use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};

use common::*;
use extrema_infra::{arch::market_assets::api_data::utils_data::InstrumentInfo, prelude::*};
use perp_dex::prelude::*;
use reqwest::Client;

const RUN_FOR: Duration = Duration::from_secs(30);
const PONG_WINDOW_RUN: Duration = Duration::from_secs(170);

fn shared_client() -> Arc<Client> {
    Arc::new(Client::new())
}

fn book25() -> WsChannel {
    WsChannel::Lob(Some(LobParam::Incremental {
        depth: Some(25),
        frequency: None,
    }))
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
        assert!(info.inst_code.is_some(), "{info:?}");
    }
}

#[tokio::test]
#[ignore = "hits the live ApeX API"]
async fn apex_public_rest() {
    let cli = ApexCli::new(shared_client());

    let config = cli.get_symbols().await.unwrap().contractConfig;
    assert!(!config.predictionContract.is_empty());
    for contract in config
        .perpetualContract
        .iter()
        .chain(&config.stockContract)
        .chain(&config.predictionContract)
    {
        assert_eq!(
            contract.inst(),
            format!("{}_{}_PERP", contract.baseTokenId, contract.settleAssetId),
            "symbol parsing disagrees with symbols for {}",
            contract.crossSymbolName
        );
        assert_eq!(
            cli_perp_to_apex_symbol(&contract.inst()).unwrap(),
            contract.crossSymbolName
        );
    }
    let stocks: Vec<_> = config
        .stockContract
        .iter()
        .filter(|c| c.is_stock())
        .collect();
    eprintln!(
        "ApeX stocks: {} {:?}",
        stocks.len(),
        stocks.iter().map(|c| &c.baseTokenId).collect::<Vec<_>>()
    );
    assert!(stocks.len() >= 20, "{} stock perps", stocks.len());
    assert!(config.perpetualContract.iter().all(|c| !c.is_stock()));
    let nvda = stocks
        .iter()
        .find(|c| c.crossSymbolName == "NVDAUSDT")
        .expect("ApeX lists NVDA");

    let perps = cli
        .get_instrument_info(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert_sane_instruments(&perps);
    assert_eq!(
        perps.len(),
        config.perpetualContract.len() + config.stockContract.len()
    );
    assert!(
        cli.get_instrument_info(InstrumentType::Spot)
            .await
            .unwrap()
            .is_empty()
    );
    let info = perps.iter().find(|i| i.inst == "NVDA_USDT_PERP").unwrap();
    assert_eq!(info.inst_code.as_deref(), Some(nvda.l2PairId.as_str()));
    assert_eq!(info.state, InstrumentStatus::Live);
    assert!(info.max_leverage.is_some_and(|l| l > 1));

    let live = cli
        .get_live_instruments(InstrumentType::Perpetual)
        .await
        .unwrap();
    for inst in ["BTC_USDT_PERP", "NVDA_USDT_PERP"] {
        assert!(live.contains(&inst.to_string()), "{inst} is not live");
    }
    assert!(
        live.len() < perps.len(),
        "ApeX keeps delisted markets listed"
    );

    let raw = cli.get_all_tickers().await.unwrap();
    let perp_insts: HashSet<String> = perps.iter().map(|i| i.inst.clone()).collect();
    assert!(raw.iter().any(|t| !perp_insts.contains(&t.inst())));
    let every = cli.get_tickers(None, None).await.unwrap();
    assert!(every.len() >= live.len());
    assert!(every.iter().all(|t| perp_insts.contains(&t.inst)));
    assert!(
        cli.get_tickers(None, Some(InstrumentType::Spot))
            .await
            .unwrap()
            .is_empty()
    );

    let insts = vec!["NVDA_USDT_PERP".to_string(), "BTC_USDT_PERP".to_string()];
    let tickers = cli
        .get_tickers(Some(&insts), Some(InstrumentType::Perpetual))
        .await
        .unwrap();
    assert_eq!(tickers.len(), 2);
    assert!(tickers.iter().all(|t| t.price > 0.0
        && t.inst_type == InstrumentType::Perpetual
        && t.timestamp > 1_700_000_000_000_000));
    let marks = cli.get_mark_prices(Some(&insts), None).await.unwrap();
    assert_eq!(marks.len(), 2);
    for mark in &marks {
        let last = tickers.iter().find(|t| t.inst == mark.inst).unwrap().price;
        assert!(
            (mark.mark_price / last - 1.0).abs() < 0.05,
            "{mark:?} vs {last}"
        );
    }
    assert!(cli.get_mark_prices(None, None).await.unwrap().len() >= live.len());

    let book = cli
        .get_orderbook("NVDA_USDT_PERP", InstrumentType::Perpetual, 5)
        .await
        .unwrap();
    assert_sane_book(&book, "NVDA_USDT_PERP", 5);
    let deep = cli
        .get_orderbook("BTC_USDT_PERP", InstrumentType::Perpetual, 0)
        .await
        .unwrap();
    assert_sane_book(&deep, "BTC_USDT_PERP", 200);
    assert!(deep.bids.len() > 25);
    let mid = (deep.bids[0].0 + deep.asks[0].0) / 2.0;
    let btc = tickers.iter().find(|t| t.inst == "BTC_USDT_PERP").unwrap();
    assert!((btc.price / mid - 1.0).abs() < 0.01, "{btc:?} vs {mid}");

    assert!(
        cli.get_orderbook("NVDA_USDT_PERP", InstrumentType::Perpetual, 201)
            .await
            .is_err()
    );
    assert!(
        cli.get_orderbook("NVDA_USDT_PERP", InstrumentType::Spot, 5)
            .await
            .is_err()
    );
    assert!(
        cli.get_orderbook("NVDAUSDT", InstrumentType::Perpetual, 5)
            .await
            .is_err()
    );
    let err = cli
        .get_orderbook("NOPE_USDT_PERP", InstrumentType::Perpetual, 5)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("no order book"), "{err}");
}

#[tokio::test]
#[ignore = "hits the live ApeX API"]
async fn perp_dex_clients_dispatch_to_apex() {
    let venue = PerpDexClients::Apex(ApexCli::new(shared_client()));
    let inst = "NVDA_USDT_PERP";
    assert_eq!(venue.market(), APEX);

    let infos = venue
        .get_instrument_info(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert!(infos.iter().any(|i| i.inst == inst));

    let book = venue
        .get_orderbook(inst, InstrumentType::Perpetual, 5)
        .await
        .unwrap();
    assert_sane_book(&book, inst, 5);

    let marks = venue
        .get_mark_prices(Some(&[inst.to_string()]), Some(InstrumentType::Perpetual))
        .await
        .unwrap();
    assert_eq!(marks.len(), 1);
    let mid = (book.bids[0].0 + book.asks[0].0) / 2.0;
    assert!((marks[0].mark_price / mid - 1.0).abs() < 0.05);

    let channel = WsChannel::Lob(None);
    assert_eq!(
        venue.get_public_connect_msg(&channel).await.unwrap(),
        "wss://quote.omni.apex.exchange/realtime_public?v=2"
    );
    assert!(
        venue
            .get_public_sub_msg(&channel, Some(&[inst.to_string()]))
            .await
            .unwrap()
            .contains("orderBook200.H.NVDAUSDT")
    );
    assert!(matches!(
        venue
            .get_private_connect_msg(&WsChannel::AccountOrders)
            .await,
        Err(InfraError::Unimplemented)
    ));
}

/// Positive `f64` bit patterns sort like the values.
#[derive(Default)]
struct LocalBook {
    bids: BTreeMap<u64, f64>,
    asks: BTreeMap<u64, f64>,
}

impl LocalBook {
    fn apply(&mut self, lob: &WsLob) {
        if matches!(lob.event, LobEventKind::Snapshot) {
            self.bids.clear();
            self.asks.clear();
        }
        for (side, levels) in [(&mut self.bids, &lob.bids), (&mut self.asks, &lob.asks)] {
            for level in levels {
                match level.action {
                    LobLevelAction::Delete => side.remove(&level.price.to_bits()),
                    LobLevelAction::Upsert => side.insert(level.price.to_bits(), level.size),
                };
            }
        }
    }

    fn bbo(&self) -> (f64, f64) {
        let bid = self
            .bids
            .keys()
            .next_back()
            .map_or(0.0, |p| f64::from_bits(*p));
        let ask = self.asks.keys().next().map_or(0.0, |p| f64::from_bits(*p));
        (bid, ask)
    }
}

/// Rebuilds the book from snapshot and deltas, checking it never crosses.
fn replay_book(lobs: &[WsLob], inst: &str, max_levels: usize) -> (f64, f64) {
    assert!(!lobs.is_empty(), "no book events for {inst}");
    assert!(matches!(lobs[0].event, LobEventKind::Snapshot));
    assert_book_side_order(&lobs[0]);
    assert!(
        lobs[1..]
            .iter()
            .all(|l| !matches!(l.event, LobEventKind::Snapshot | LobEventKind::Bbo)),
        "unexpected resnapshot"
    );
    assert_last_contiguous(lobs);
    assert!(
        lobs.iter()
            .all(|l| l.market == APEX && l.inst == inst && l.timestamp > 1_700_000_000_000_000)
    );
    assert!(lobs.windows(2).all(|w| w[0].timestamp <= w[1].timestamp));

    let mut book = LocalBook::default();
    for lob in lobs {
        book.apply(lob);
        let (bid, ask) = book.bbo();
        assert!(
            0.0 < bid && bid < ask,
            "{inst} crossed or empty after {lob:?}"
        );
        assert!(book.bids.len() <= max_levels && book.asks.len() <= max_levels);
    }
    book.bbo()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "streams from the live ApeX and Aster venues for 30 seconds"]
async fn apex_live_streams_decode_cleanly() {
    let apex = || PerpDexClients::Apex(ApexCli::default());
    let raw = || WsChannel::Other("instrumentInfo.H".into());
    let cases = vec![
        Case::new(1, apex(), WsChannel::Lob(None), "BTC_USDT_PERP", "")
            .with_keepalive(apex_keepalive()),
        Case::new(2, apex(), book25(), "BTC_USDT_PERP", "").with_keepalive(apex_keepalive()),
        Case::new(3, apex(), WsChannel::Trades(None), "BTC_USDT_PERP", "")
            .with_keepalive(apex_keepalive()),
        Case::new(4, apex(), raw(), "BTC_USDT_PERP", ""),
        Case::new(5, apex(), WsChannel::Lob(None), "NVDA_USDT_PERP", "")
            .with_keepalive(apex_keepalive()),
        Case::new(6, apex(), book25(), "NVDA_USDT_PERP", "").with_keepalive(apex_keepalive()),
        Case::new(7, apex(), WsChannel::Trades(None), "NVDA_USDT_PERP", "")
            .with_keepalive(apex_keepalive()),
        Case::new(8, apex(), raw(), "NVDA_USDT_PERP", ""),
        Case::new(
            9,
            PerpDexClients::Aster(AsterCli::default()),
            bbo(),
            "NVDA_USDT_PERP",
            "",
        ),
    ];

    let run = live(cases, RUN_FOR).await;
    eprintln!("{}", run.summary());

    assert_clean(&run);
    assert!(
        run.connects.len() == 9 && run.connects.values().all(|n| *n == 1),
        "reconnected: {:?}",
        run.connects
    );

    let btc = replay_book(run.lobs(1), "BTC_USDT_PERP", 200);
    let btc25 = replay_book(run.lobs(2), "BTC_USDT_PERP", 25);
    let nvda = replay_book(run.lobs(5), "NVDA_USDT_PERP", 200);
    let nvda25 = replay_book(run.lobs(6), "NVDA_USDT_PERP", 25);
    eprintln!("ApeX BBO: BTC {btc:?} / {btc25:?}, NVDA {nvda:?} / {nvda25:?}");
    for (a, b) in [(btc, btc25), (nvda, nvda25)] {
        assert!(
            ((a.0 + a.1) / (b.0 + b.1) - 1.0).abs() < 0.005,
            "{a:?} vs {b:?}"
        );
    }

    assert!(!run.trades(3).is_empty(), "no BTC trades in {RUN_FOR:?}");
    for (id, inst) in [(3, "BTC_USDT_PERP"), (7, "NVDA_USDT_PERP")] {
        let trades = run.trades(id);
        assert!(trades.iter().all(|t| t.market == APEX
            && t.inst == inst
            && matches!(t.side, OrderSide::BUY | OrderSide::SELL)
            && t.price > 0.0
            && t.size > 0.0
            && t.timestamp > 1_700_000_000_000_000));
        assert_eq!(
            trades
                .iter()
                .map(|t| t.trade_id)
                .collect::<HashSet<_>>()
                .len(),
            trades.len()
        );
    }
    assert!(run.others.contains(&4) && run.others.contains(&8));

    let aster = run.lobs(9);
    assert!(!aster.is_empty(), "no Aster NVDA BBO");
    aster.iter().for_each(assert_bbo);
    let aster_mid = mid(aster.last().unwrap());
    let apex_mid = (nvda.0 + nvda.1) / 2.0;
    eprintln!("NVDA mids (ApeX, Aster): {apex_mid} {aster_mid}");
    assert!(
        (apex_mid / aster_mid - 1.0).abs() < 0.02,
        "NVDA mids disagree: {apex_mid} vs {aster_mid}"
    );
}

/// ApeX drops a connection about 150 s after the client's last pong; `apex_keepalive` keeps it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "streams from the live ApeX venue for 170 seconds"]
async fn apex_keepalive_outlives_the_pong_window() {
    let apex = || PerpDexClients::Apex(ApexCli::default());
    let cases = vec![
        Case::new(1, apex(), book25(), "BTC_USDT_PERP", "").with_keepalive(apex_keepalive()),
        Case::new(2, apex(), book25(), "ETH_USDT_PERP", ""),
    ];

    let run = live(cases, PONG_WINDOW_RUN).await;
    eprintln!("{}", run.summary());
    eprintln!(
        "without keepalive the ETH task connected {} times",
        run.connects.get(&2).copied().unwrap_or_default()
    );

    assert_clean(&run);
    assert_eq!(run.connects.get(&1), Some(&1), "{:?}", run.connects);
    let book = run.lobs(1);
    let span = book.last().unwrap().timestamp - book[0].timestamp;
    assert!(span > 155_000_000, "book stopped after {span} us");
    replay_book(book, "BTC_USDT_PERP", 25);
}
