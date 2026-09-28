//! Extended against the live venue: `cargo test --test extended_live -- --ignored`.

mod common;

use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
    time::Duration,
};

use common::*;
use extrema_infra::{
    arch::market_assets::{
        api_data::utils_data::InstrumentInfo, api_general::get_micros_timestamp,
    },
    prelude::*,
};
use perp_dex::prelude::*;
use reqwest::Client;

const RUN_FOR: Duration = Duration::from_secs(30);
/// Long enough for every book to pass a minutely snapshot after the one it opens with.
const SNAPSHOT_EVERY: Duration = Duration::from_secs(75);
const BTC: &str = "BTC_USD_PERP";
const NVDA: &str = "NVDA_24_5_USD_PERP";

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
        assert!(info.max_leverage.is_some_and(|l| l >= 1), "{info:?}");
    }
}

#[tokio::test]
#[ignore = "hits the live Extended API"]
async fn extended_public_rest() {
    let client = Arc::new(Client::new());
    let cli = ExtendedCli::new(client.clone());

    let markets = cli.get_markets(None).await.unwrap();
    for market in &markets {
        match market.inst_type() {
            InstrumentType::Perpetual => assert_eq!(
                cli_perp_to_extended_market(&market.inst()).unwrap(),
                market.name
            ),
            InstrumentType::Spot => assert_eq!(
                market.inst(),
                format!("{}_{}", market.assetName, market.collateralAssetName)
            ),
            other => panic!("{} is {other:?}", market.name),
        }
        if market.name != format!("{}-{}", market.assetName, market.collateralAssetName) {
            assert_eq!(
                market.status, "DELISTED",
                "{} is not <asset>-<collateral>",
                market.name
            );
        }
    }
    let equities: Vec<_> = markets
        .iter()
        .filter(|m| m.is_equity() && m.status == "ACTIVE")
        .collect();
    assert!(equities.len() > 50, "{} equity perps", equities.len());
    let nvda = equities.iter().find(|m| m.uiName == "NVDA-USD").unwrap();
    assert_eq!(
        (nvda.name.as_str(), nvda.inst()),
        ("NVDA_24_5-USD", NVDA.to_string())
    );

    let filtered = cli
        .get_markets(Some(&[NVDA.to_string(), BTC.to_string()]))
        .await
        .unwrap();
    assert_eq!(
        filtered.iter().map(|m| m.inst()).collect::<HashSet<_>>(),
        HashSet::from([NVDA.to_string(), BTC.to_string()])
    );
    let err = cli
        .get_markets(Some(&["NOPE_USD_PERP".to_string()]))
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("Extended REST error")
            && err.to_string().contains("Market not found"),
        "{err}"
    );
    assert!(matches!(
        cli.get_markets(Some(&["NVDA-USD".to_string()])).await,
        Err(InfraError::ApiCliError(_))
    ));

    let perp_count = markets
        .iter()
        .filter(|m| m.inst_type() == InstrumentType::Perpetual)
        .count();
    let perps = cli
        .get_instrument_info(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert_sane_instruments(&perps);
    assert_eq!(perps.len(), perp_count);
    let info = perps.iter().find(|i| i.inst == NVDA).unwrap();
    assert_eq!(info.state, InstrumentStatus::Live);
    assert!(info.inst_code.is_none() && info.min_notional.is_none());

    let spots = cli.get_instrument_info(InstrumentType::Spot).await.unwrap();
    assert!(!spots.is_empty() && spots.len() == markets.len() - perp_count);
    assert!(
        spots
            .iter()
            .all(|i| i.inst_type == InstrumentType::Spot && !i.inst.ends_with("_PERP"))
    );

    let live = cli
        .get_live_instruments(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert!(live.contains(&NVDA.to_string()) && live.contains(&BTC.to_string()));
    assert_eq!(
        live.len(),
        markets
            .iter()
            .filter(|m| m.inst_type() == InstrumentType::Perpetual
                && m.status == "ACTIVE"
                && !m.isOffHours)
            .count()
    );
    assert!(
        live.len() < perps.len(),
        "some markets are delisted or prelisted"
    );

    let insts = vec![NVDA.to_string(), BTC.to_string()];
    let tickers = cli.get_tickers(Some(&insts), None).await.unwrap();
    assert_eq!(tickers.len(), 2);
    assert!(
        tickers
            .iter()
            .all(|t| t.price > 0.0 && t.timestamp > 1_700_000_000_000_000)
    );
    let spot_tickers = cli
        .get_tickers(None, Some(InstrumentType::Spot))
        .await
        .unwrap();
    assert!(
        !spot_tickers.is_empty()
            && spot_tickers
                .iter()
                .all(|t| t.inst_type == InstrumentType::Spot)
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
    let all_marks: HashSet<String> = cli
        .get_mark_prices(None, None)
        .await
        .unwrap()
        .into_iter()
        .map(|m| m.inst)
        .collect();
    assert!(
        live.iter().all(|inst| all_marks.contains(inst)),
        "a live perp has no mark price"
    );
    assert!(
        cli.get_mark_prices(None, Some(InstrumentType::Spot))
            .await
            .unwrap()
            .is_empty(),
        "spot markets have no mark price"
    );

    let book = cli
        .get_orderbook(NVDA, InstrumentType::Perpetual, 5)
        .await
        .unwrap();
    assert_sane_book(&book, NVDA, 5);
    let deep = cli
        .get_orderbook(BTC, InstrumentType::Perpetual, 0)
        .await
        .unwrap();
    assert_sane_book(&deep, BTC, usize::MAX);
    assert!(deep.bids.len() > 100 && deep.asks.len() > 100);
    assert!(
        cli.get_orderbook(NVDA, InstrumentType::Spot, 5)
            .await
            .is_err()
    );
    assert!(
        cli.get_orderbook("NVDA-USD", InstrumentType::Perpetual, 5)
            .await
            .is_err()
    );
    let err = cli
        .get_orderbook("NOPE_USD_PERP", InstrumentType::Perpetual, 5)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("no data"), "{err}");

    let venue = PerpDexClients::Extended(ExtendedCli::new(client));
    assert_eq!(venue.market(), EXTENDED);
    assert!(
        venue
            .get_instrument_info(InstrumentType::Perpetual)
            .await
            .unwrap()
            .iter()
            .any(|i| i.inst == NVDA)
    );
    let book = venue
        .get_orderbook(NVDA, InstrumentType::Perpetual, 5)
        .await
        .unwrap();
    assert_sane_book(&book, NVDA, 5);
    let marks = venue
        .get_mark_prices(Some(&[NVDA.to_string()]), Some(InstrumentType::Perpetual))
        .await
        .unwrap();
    assert_eq!(marks.len(), 1);
    let mid = (book.bids[0].0 + book.asks[0].0) / 2.0;
    assert!(
        (marks[0].mark_price / mid - 1.0).abs() < 0.05,
        "mark far from mid"
    );
    assert!(matches!(
        venue.get_public_connect_msg(&bbo()).await,
        Err(InfraError::ApiCliError(_))
    ));
}

/// A local book rebuilt from the stream: never crossed, and equal to each minutely snapshot.
#[derive(Default)]
struct LocalBook {
    bids: BTreeMap<u64, f64>,
    asks: BTreeMap<u64, f64>,
    reconciled: usize,
}

impl LocalBook {
    fn side(levels: &[LobLevel]) -> BTreeMap<u64, f64> {
        levels.iter().map(|l| (l.price.to_bits(), l.size)).collect()
    }

    fn apply(&mut self, lob: &WsLob) {
        match lob.event {
            LobEventKind::Snapshot => {
                let (bids, asks) = (Self::side(&lob.bids), Self::side(&lob.asks));
                if !self.bids.is_empty() {
                    assert!(
                        self.bids == bids && self.asks == asks,
                        "book drifted from snapshot {:?}",
                        lob.seq
                    );
                    self.reconciled += 1;
                }
                (self.bids, self.asks) = (bids, asks);
            },
            LobEventKind::Incremental => {
                for (side, levels) in [(&mut self.bids, &lob.bids), (&mut self.asks, &lob.asks)] {
                    for level in levels {
                        match level.action {
                            LobLevelAction::Delete => side.remove(&level.price.to_bits()),
                            _ => side.insert(level.price.to_bits(), level.size),
                        };
                    }
                }
            },
            LobEventKind::Heartbeat => {},
            _ => panic!("unexpected {lob:?}"),
        }
        if let (Some((bid, _)), Some((ask, _))) =
            (self.bids.last_key_value(), self.asks.first_key_value())
        {
            assert!(
                f64::from_bits(*bid) < f64::from_bits(*ask),
                "crossed at {:?}",
                lob.seq
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "streams from live Extended and Aster for 30 seconds"]
async fn extended_live_streams_decode_cleanly() {
    let extended = PerpDexClients::Extended(ExtendedCli::default());
    let cases = vec![
        Case::new(1, extended.clone(), WsChannel::Lob(None), BTC, ""),
        Case::new(2, extended.clone(), bbo(), BTC, ""),
        Case::new(3, extended.clone(), WsChannel::Trades(None), BTC, ""),
        Case::new(
            4,
            extended.clone(),
            WsChannel::Lob(Some(LobParam::Incremental {
                depth: None,
                frequency: Some(LobFrequency::Ms100),
            })),
            NVDA,
            "",
        ),
        Case::new(5, extended.clone(), bbo(), NVDA, ""),
        Case::new(
            6,
            extended.clone(),
            WsChannel::Trades(Some(TradesParam::AllTrades)),
            NVDA,
            "",
        ),
        Case::new(7, extended, WsChannel::Other("prices/mark".into()), BTC, ""),
        Case::new(
            8,
            PerpDexClients::Aster(AsterCli::default()),
            bbo(),
            "NVDA_USDT_PERP",
            "",
        ),
    ];

    let started_us = get_micros_timestamp();
    let run = live(cases, RUN_FOR).await;
    eprintln!("{}", run.summary());

    assert_clean(&run);
    assert_eq!(run.connects.len(), 8);
    assert!(
        run.connects.values().all(|n| *n == 1),
        "reconnected: {:?}",
        run.connects
    );
    for id in [1, 2, 4, 5, 8] {
        assert!(!run.lobs(id).is_empty(), "no book events on task {id}");
    }
    assert!(run.others.contains(&7), "no mark price frames");

    for (id, inst) in [(1, BTC), (4, NVDA)] {
        let book = run.lobs(id);
        assert!(matches!(book[0].event, LobEventKind::Snapshot));
        assert_eq!(book[0].seq.as_ref().unwrap().last, Some(1));
        assert_book_side_order(&book[0]);
        assert_last_contiguous(book);
        let mut local = LocalBook::default();
        book.iter().for_each(|lob| local.apply(lob));
        eprintln!(
            "{inst}: {} book events, {} minutely snapshots reconciled",
            book.len(),
            local.reconciled
        );
        assert!(book.iter().all(|l| l.market == EXTENDED
            && l.inst == inst
            && l.timestamp > 1_700_000_000_000_000));
    }

    for (id, inst) in [(2, BTC), (5, NVDA)] {
        let bbo = run.lobs(id);
        bbo.iter().for_each(assert_bbo);
        assert_eq!(bbo[0].seq.as_ref().unwrap().last, Some(1));
        assert_last_contiguous(bbo);
        assert!(bbo.iter().all(|l| l.market == EXTENDED
            && l.inst == inst
            && l.timestamp > 1_700_000_000_000_000));
    }

    assert!(!run.trades(3).is_empty(), "no BTC trades in {RUN_FOR:?}");
    for (id, inst) in [(3, BTC), (6, NVDA)] {
        let trades = run.trades(id);
        assert!(trades.iter().all(|t| t.market == EXTENDED
            && t.inst == inst
            && matches!(t.side, OrderSide::BUY | OrderSide::SELL)
            && t.price > 0.0
            && t.size > 0.0));
        assert!(
            trades.iter().all(|t| t.timestamp + 10_000_000 > started_us),
            "replayed history reached on_trade"
        );
        assert_eq!(
            trades
                .iter()
                .map(|t| t.trade_id)
                .collect::<HashSet<_>>()
                .len(),
            trades.len()
        );
    }

    let extended_mid = mid(run.lobs(5).last().unwrap());
    let aster_mid = mid(run.lobs(8).last().unwrap());
    eprintln!("NVDA mids (Extended, Aster): {extended_mid} {aster_mid}");
    assert!(
        (extended_mid / aster_mid - 1.0).abs() < 0.02,
        "NVDA mids disagree: {extended_mid} vs {aster_mid}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "streams from live Extended for 75 seconds"]
async fn extended_books_rebuild_to_the_minutely_snapshot() {
    let extended = PerpDexClients::Extended(ExtendedCli::default());
    let cases = vec![
        Case::new(1, extended.clone(), WsChannel::Lob(None), BTC, ""),
        Case::new(2, extended, WsChannel::Lob(None), NVDA, ""),
    ];

    let run = live(cases, SNAPSHOT_EVERY).await;
    eprintln!("{}", run.summary());

    assert_clean(&run);
    assert!(run.connects.values().all(|n| *n == 1), "{:?}", run.connects);
    for id in [1, 2] {
        let book = run.lobs(id);
        assert_last_contiguous(book);
        let mut local = LocalBook::default();
        book.iter().for_each(|lob| local.apply(lob));
        assert!(local.reconciled >= 1, "task {id} saw no minutely snapshot");
        eprintln!(
            "task {id}: {} minutely snapshots matched the rebuilt book",
            local.reconciled
        );
    }
}
