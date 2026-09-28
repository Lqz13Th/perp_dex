//! edgeX against the live venue: `cargo test --test edgex_live -- --ignored`.

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

const BTC: &str = "@30000001";
const NVDA: &str = "@30000020";
const RUN_FOR: Duration = Duration::from_secs(30);

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
        assert!(info.max_leverage.is_some_and(|l| l > 0), "{info:?}");
    }
}

#[tokio::test]
#[ignore = "hits the live edgeX API"]
async fn edgex_public_rest() {
    let cli = EdgexCli::new(Arc::new(Client::new()));

    let meta = cli.get_meta_data().await.unwrap();
    let contracts = &meta.contractList;
    assert!(contracts.len() > 100, "{} contracts", contracts.len());
    for contract in contracts {
        assert_eq!(
            cli_to_edgex_contract_id(&contract.inst()).unwrap(),
            contract.contractId
        );
        assert_eq!(edgex_contract_to_cli(contract.contractId), contract.inst());
        assert!(meta.coin_name(&contract.baseCoinId).is_some());
        assert_eq!(meta.coin_name(&contract.quoteCoinId), Some("USDC"));
    }
    let stocks: Vec<_> = contracts.iter().filter(|c| c.is_stock()).collect();
    eprintln!(
        "edgeX: {} contracts, {} stocks",
        contracts.len(),
        stocks.len()
    );
    assert!(stocks.len() > 50);
    let nvda = stocks
        .iter()
        .find(|c| meta.coin_name(&c.baseCoinId) == Some("NVDA"))
        .expect("edgeX lists NVDA");
    assert_eq!(
        (nvda.inst(), nvda.contractName.as_str()),
        (NVDA.to_string(), "NVDAUSDC")
    );
    assert!(
        contracts
            .iter()
            .filter(|c| ["BTC", "XAU", "EURUSD"].contains(&meta.coin_name(&c.baseCoinId).unwrap()))
            .all(|c| !c.is_stock())
    );

    let perps = cli
        .get_instrument_info(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert_sane_instruments(&perps);
    assert_eq!(perps.len(), contracts.len());
    assert_eq!(
        perps
            .iter()
            .map(|i| (i.inst.clone(), i.inst_code.clone().unwrap()))
            .collect::<HashSet<_>>(),
        contracts
            .iter()
            .map(|c| (c.inst(), c.contractName.clone()))
            .collect()
    );
    assert!(
        cli.get_instrument_info(InstrumentType::Spot)
            .await
            .unwrap()
            .is_empty()
    );

    let live = cli
        .get_live_instruments(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert!(live.contains(&BTC.to_string()) && live.contains(&NVDA.to_string()));
    assert_eq!(
        live.len(),
        contracts
            .iter()
            .filter(|c| c.enableTrade && c.enableOpenPosition)
            .count()
    );

    let insts = vec![NVDA.to_string(), BTC.to_string()];
    let tickers = cli.get_tickers(Some(&insts), None).await.unwrap();
    assert_eq!(
        tickers
            .iter()
            .map(|t| t.inst.clone())
            .collect::<HashSet<_>>(),
        insts.iter().cloned().collect()
    );
    assert!(
        tickers
            .iter()
            .all(|t| t.price > 0.0 && t.timestamp > 1_700_000_000_000_000)
    );
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
    }

    let all_tickers = cli.get_tickers(None, None).await.unwrap();
    let all_marks = cli.get_mark_prices(None, None).await.unwrap();
    eprintln!(
        "edgeX: {} tickers, {} marks over {} contracts",
        all_tickers.len(),
        all_marks.len(),
        contracts.len()
    );
    let perp_insts: HashSet<_> = perps.iter().map(|i| i.inst.clone()).collect();
    assert!(all_tickers.len() * 10 >= contracts.len() * 9);
    assert!(all_marks.len() >= all_tickers.len());
    assert!(
        all_tickers
            .iter()
            .map(|t| &t.inst)
            .chain(all_marks.iter().map(|m| &m.inst))
            .all(|inst| perp_insts.contains(inst))
    );
    assert!(
        cli.get_tickers(None, Some(InstrumentType::Spot))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        cli.get_tickers(Some(&["@99999999".to_string()]), None)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        cli.get_mark_prices(Some(&["NVDAUSDC".to_string()]), None)
            .await
            .is_err()
    );

    let book = cli
        .get_orderbook(NVDA, InstrumentType::Perpetual, 15)
        .await
        .unwrap();
    assert_sane_book(&book, NVDA, 15);
    let deep = cli
        .get_orderbook(BTC, InstrumentType::Perpetual, 0)
        .await
        .unwrap();
    assert_sane_book(&deep, BTC, 200);
    assert!(deep.bids.len() > 15);
    for (inst, inst_type, depth) in [
        (NVDA, InstrumentType::Perpetual, 5),
        (NVDA, InstrumentType::Spot, 15),
        ("NVDAUSDC", InstrumentType::Perpetual, 15),
    ] {
        assert!(
            cli.get_orderbook(inst, inst_type, depth).await.is_err(),
            "{inst} {depth}"
        );
    }
    let err = cli
        .get_orderbook("@99999999", InstrumentType::Perpetual, 15)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("no contract"), "{err}");

    let venue = PerpDexClients::Edgex(cli.clone());
    assert_eq!(venue.market(), EDGEX);
    let book = venue
        .get_orderbook(NVDA, InstrumentType::Perpetual, 200)
        .await
        .unwrap();
    assert_sane_book(&book, NVDA, 200);
    assert!(
        venue
            .get_instrument_info(InstrumentType::Perpetual)
            .await
            .unwrap()
            .iter()
            .any(|i| i.inst == NVDA)
    );
}

/// Applies snapshots and deltas the way a strategy keeps its book.
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
                let key = (level.price * 1e8).round() as u64;
                match level.action {
                    LobLevelAction::Delete => side.remove(&key),
                    LobLevelAction::Upsert => side.insert(key, level.size),
                };
            }
        }
    }

    fn best(&self) -> Option<((f64, f64), (f64, f64))> {
        let (bid, bid_size) = self.bids.last_key_value()?;
        let (ask, ask_size) = self.asks.first_key_value()?;
        Some((
            (*bid as f64 / 1e8, *bid_size),
            (*ask as f64 / 1e8, *ask_size),
        ))
    }

    fn notional(&self, levels: usize) -> (f64, f64) {
        let sum = |iter: &mut dyn Iterator<Item = (&u64, &f64)>| {
            iter.take(levels)
                .map(|(price, size)| *price as f64 / 1e8 * size)
                .sum::<f64>()
        };
        (sum(&mut self.bids.iter().rev()), sum(&mut self.asks.iter()))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "streams from edgeX and Aster for 30 seconds"]
async fn edgex_live_streams_decode_cleanly() {
    let edgex = PerpDexClients::Edgex(EdgexCli::default());
    let depth15 = WsChannel::Lob(Some(LobParam::Incremental {
        depth: Some(15),
        frequency: None,
    }));
    let cases = vec![
        Case::new(1, edgex.clone(), depth15.clone(), BTC, ""),
        Case::new(2, edgex.clone(), WsChannel::Lob(None), BTC, ""),
        Case::new(3, edgex.clone(), depth15, NVDA, ""),
        Case::new(4, edgex.clone(), WsChannel::Lob(None), NVDA, ""),
        Case::new(5, edgex.clone(), WsChannel::Trades(None), BTC, ""),
        Case::new(6, edgex.clone(), WsChannel::Trades(None), NVDA, ""),
        Case::new(7, edgex.clone(), bbo(), "", ""),
        Case::new(8, edgex, WsChannel::Other("ticker".into()), NVDA, ""),
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
    assert!(run.others.contains(&8));

    let mut books = Vec::new();
    for (task, inst, max_levels) in [(1, BTC, 15), (2, BTC, 200), (3, NVDA, 15), (4, NVDA, 200)] {
        let lobs = run.lobs(task);
        assert!(lobs.len() > 1, "no book updates on task {task}");
        assert!(matches!(lobs[0].event, LobEventKind::Snapshot));
        assert_book_side_order(&lobs[0]);
        assert!(
            lobs[1..]
                .iter()
                .all(|l| !matches!(l.event, LobEventKind::Snapshot))
        );
        assert_prev_chain(lobs);
        assert!(
            lobs.iter()
                .all(|l| l.market == EDGEX && l.inst == inst && l.timestamp == 0)
        );

        let mut book = LocalBook::default();
        for lob in lobs {
            book.apply(lob);
            assert!(book.bids.len() <= max_levels && book.asks.len() <= max_levels);
            let ((bid, _), (ask, _)) = book.best().expect("two-sided book");
            assert!(bid < ask, "crossed local book on task {task}");
        }
        books.push(book);
    }

    let all_bbo = run.lobs(7);
    let bbo_of =
        |inst: &str| -> Vec<&WsLob> { all_bbo.iter().filter(|l| l.inst == inst).collect() };
    assert!(
        all_bbo
            .iter()
            .map(|l| &l.inst)
            .collect::<HashSet<_>>()
            .len()
            > 100,
        "BBO covers every market"
    );
    for inst in [BTC, NVDA] {
        let bbo = bbo_of(inst);
        assert!(!bbo.is_empty(), "no {inst} BBO");
        for lob in &bbo {
            assert_bbo(lob);
            assert!(lob.market == EDGEX && lob.timestamp > 1_700_000_000_000_000);
        }
    }

    let aster = run.lobs(9);
    aster.iter().for_each(assert_bbo);
    let edgex_mid = mid(bbo_of(NVDA).last().unwrap());
    let aster_mid = mid(aster.last().expect("Aster NVDA BBO"));
    eprintln!("NVDA mids (edgeX, Aster): {edgex_mid} {aster_mid}");
    assert!(
        (edgex_mid / aster_mid - 1.0).abs() < 0.02,
        "NVDA mids disagree: {edgex_mid} vs {aster_mid}"
    );

    let nvda_bbo = bbo_of(NVDA);
    let last = nvda_bbo.last().unwrap();
    let ((bid, bid_size), (ask, ask_size)) = books[2].best().unwrap();
    let (bid5, ask5) = books[2].notional(5);
    let (bid15, ask15) = books[2].notional(15);
    let spreads: Vec<f64> = nvda_bbo
        .iter()
        .map(|l| (l.asks[0].price - l.bids[0].price) / mid(l) * 1e4)
        .collect();
    eprintln!(
        "edgeX NVDA top of book: {bid_size} @ {bid} / {ask_size} @ {ask} (BBO stream {} @ {} / {} @ {}); \
         spread {:.1}-{:.1} bps over {} BBOs; top-5 notional ${bid5:.0} / ${ask5:.0}, top-15 ${bid15:.0} / ${ask15:.0}",
        last.bids[0].size,
        last.bids[0].price,
        last.asks[0].size,
        last.asks[0].price,
        spreads.iter().cloned().fold(f64::MAX, f64::min),
        spreads.iter().cloned().fold(f64::MIN, f64::max),
        spreads.len(),
    );

    assert!(!run.trades(5).is_empty(), "no BTC trades in {RUN_FOR:?}");
    for (task, inst) in [(5, BTC), (6, NVDA)] {
        let trades = run.trades(task);
        assert!(trades.iter().all(|t| t.market == EDGEX
            && t.inst == inst
            && t.price > 0.0
            && t.size > 0.0
            && t.timestamp > 1_700_000_000_000_000
            && matches!(t.side, OrderSide::BUY | OrderSide::SELL)));
        assert_eq!(
            trades
                .iter()
                .map(|t| t.trade_id)
                .collect::<HashSet<_>>()
                .len(),
            trades.len()
        );
    }
    eprintln!(
        "edgeX trades in {RUN_FOR:?}: BTC {}, NVDA {}",
        run.trades(5).len(),
        run.trades(6).len()
    );
}
