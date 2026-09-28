//! Nado against the live venue: `cargo test --test nado_live -- --ignored`.

mod common;

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use common::*;
use extrema_infra::prelude::*;
use perp_dex::prelude::*;
use reqwest::Client;

const BTC: &str = "@2";
const NVDA: &str = "@112";
const KBTC: &str = "@1";
const RUN_FOR: Duration = Duration::from_secs(30);

fn assert_sides_sorted(lob: &WsLob) {
    assert!(
        lob.bids.windows(2).all(|w| w[0].price > w[1].price),
        "{lob:?}"
    );
    assert!(
        lob.asks.windows(2).all(|w| w[0].price < w[1].price),
        "{lob:?}"
    );
}

#[tokio::test]
#[ignore = "hits the live Nado API"]
async fn nado_public_rest() {
    let cli = NadoCli::new(Arc::new(Client::new()));

    let symbols = cli.get_symbols().await.unwrap();
    assert!(symbols.len() > 50, "{} products", symbols.len());
    assert!(
        symbols
            .windows(2)
            .all(|w| w[0].product_id < w[1].product_id)
    );
    for s in &symbols {
        assert_eq!(cli_to_nado_product_id(&s.inst()).unwrap(), s.product_id);
        assert_eq!(nado_product_to_cli(s.product_id), s.inst());
        assert_eq!(
            s.symbol.ends_with("-PERP"),
            s.inst_type() == InstrumentType::Perpetual,
            "{s:?}"
        );
        assert!(
            matches!(
                s.inst_type(),
                InstrumentType::Perpetual | InstrumentType::Spot
            ),
            "{s:?}"
        );
        assert_ne!(s.state(), InstrumentStatus::Unknown, "{s:?}");
        for raw in [
            &s.price_increment_x18,
            &s.size_increment,
            &s.min_size,
            &s.maker_fee_rate_x18,
            &s.taker_fee_rate_x18,
            &s.long_weight_initial_x18,
            &s.long_weight_maintenance_x18,
        ] {
            assert!(nado_x18_to_decimal(raw).is_some(), "{s:?}");
        }
    }
    let by_id: HashMap<u32, _> = symbols.iter().map(|s| (s.product_id, s)).collect();
    assert_eq!(by_id.len(), symbols.len());
    assert_eq!(by_id[&2].symbol, "BTC-PERP");
    assert_eq!(by_id[&112].symbol, "NVDA-PERP");
    assert_eq!(by_id[&1].inst_type(), InstrumentType::Spot);

    let perps = cli
        .get_instrument_info(InstrumentType::Perpetual)
        .await
        .unwrap();
    let spots = cli.get_instrument_info(InstrumentType::Spot).await.unwrap();
    assert_sane_instruments(&perps);
    assert_sane_instruments(&spots);
    assert_eq!(perps.len() + spots.len(), symbols.len());
    assert!(spots.iter().all(|i| i.inst_type == InstrumentType::Spot));
    assert!(
        cli.get_instrument_info(InstrumentType::Futures)
            .await
            .unwrap()
            .is_empty()
    );
    for info in perps.iter().chain(&spots) {
        let symbol = by_id[&cli_to_nado_product_id(&info.inst).unwrap()];
        assert_eq!(info.inst_code.as_deref(), Some(symbol.symbol.as_str()));
        assert_eq!(info.state, symbol.state());
        assert!(info.min_notional.is_some_and(|n| n > 0.0), "{info:?}");
    }
    let btc = perps.iter().find(|i| i.inst == BTC).unwrap();
    assert!(btc.max_leverage.is_some_and(|l| l >= 10), "{btc:?}");
    assert!(spots.iter().all(|i| i.max_leverage.is_none()));

    let live = cli
        .get_live_instruments(InstrumentType::Perpetual)
        .await
        .unwrap();
    let expected: HashSet<String> = symbols
        .iter()
        .filter(|s| s.inst_type() == InstrumentType::Perpetual && s.trading_status == "live")
        .map(|s| s.inst())
        .collect();
    assert_eq!(live.iter().cloned().collect::<HashSet<_>>(), expected);
    assert!(live.contains(&BTC.to_string()) && live.len() <= perps.len());

    let insts = vec![BTC.to_string(), NVDA.to_string()];
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
            .all(|t| t.price > 0.0 && t.inst_type == InstrumentType::Perpetual)
    );
    let all_tickers = cli.get_tickers(None, None).await.unwrap();
    assert_eq!(
        all_tickers
            .iter()
            .map(|t| &t.inst)
            .collect::<HashSet<_>>()
            .len(),
        all_tickers.len()
    );
    for ticker in &all_tickers {
        let symbol = by_id[&cli_to_nado_product_id(&ticker.inst).unwrap()];
        assert_eq!(ticker.inst_type, symbol.inst_type(), "{ticker:?}");
        assert!(ticker.price > 0.0);
    }
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
    assert!(
        all_tickers
            .iter()
            .any(|t| t.inst_type == InstrumentType::Spot)
    );
    assert!(
        cli.get_tickers(None, Some(InstrumentType::Futures))
            .await
            .unwrap()
            .is_empty()
    );

    let marks = cli.get_mark_prices(Some(&insts), None).await.unwrap();
    assert_eq!(marks.len(), 2);
    for mark in &marks {
        let last = tickers.iter().find(|t| t.inst == mark.inst).unwrap().price;
        assert!(
            (mark.mark_price / last - 1.0).abs() < 0.05,
            "{mark:?} vs {last}"
        );
    }
    let all_marks = cli
        .get_mark_prices(None, Some(InstrumentType::Perpetual))
        .await
        .unwrap();
    assert!(all_marks.len() >= perps.len() / 2);
    for mark in &all_marks {
        let symbol = by_id[&cli_to_nado_product_id(&mark.inst).unwrap()];
        assert_eq!(symbol.inst_type(), InstrumentType::Perpetual);
        assert_eq!(mark.inst_type, InstrumentType::Perpetual);
        assert!(mark.mark_price > 0.0, "{mark:?}");
    }
    assert!(
        cli.get_mark_prices(None, Some(InstrumentType::Spot))
            .await
            .unwrap()
            .is_empty()
    );

    let book = cli
        .get_orderbook(BTC, InstrumentType::Perpetual, 5)
        .await
        .unwrap();
    assert_sane_book(&book, BTC, 5);
    assert_eq!((book.bids.len(), book.asks.len()), (5, 5));
    let deep = cli
        .get_orderbook(BTC, InstrumentType::Perpetual, 0)
        .await
        .unwrap();
    assert_sane_book(&deep, BTC, 100);
    assert!(deep.bids.len() > 5);
    let stock = cli
        .get_orderbook(NVDA, InstrumentType::Perpetual, 5)
        .await
        .unwrap();
    assert_sane_book(&stock, NVDA, 5);
    let nvda_mark = marks.iter().find(|m| m.inst == NVDA).unwrap().mark_price;
    let nvda_mid = (stock.bids[0].0 + stock.asks[0].0) / 2.0;
    assert!((nvda_mark / nvda_mid - 1.0).abs() < 0.02);
    let spot = cli
        .get_orderbook(KBTC, InstrumentType::Spot, 5)
        .await
        .unwrap();
    assert_sane_book(&spot, KBTC, 5);
    assert!((spot.bids[0].0 / book.bids[0].0 - 1.0).abs() < 0.02);

    for (inst, inst_type, depth) in [
        (BTC, InstrumentType::Perpetual, 101),
        ("BTC-PERP", InstrumentType::Perpetual, 5),
        (BTC, InstrumentType::Futures, 5),
    ] {
        assert!(matches!(
            cli.get_orderbook(inst, inst_type, depth).await,
            Err(InfraError::ApiCliError(_))
        ));
    }
    let err = cli
        .get_orderbook("@9999", InstrumentType::Perpetual, 5)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("Nado REST error") && err.contains("code=2015"),
        "{err}"
    );

    let liquidity = cli.get_market_liquidity(BTC, 3).await.unwrap();
    assert_eq!(liquidity.product_id, 2);
    assert_eq!((liquidity.bids.len(), liquidity.asks.len()), (3, 3));
    assert!(
        liquidity.timestamp > 1_700_000_000_000_000_000,
        "nanoseconds"
    );
    let ns = liquidity.timestamp;
    assert_eq!(liquidity.into_orderbook_data().timestamp, ns / 1_000);
    assert!(cli.get_market_liquidity("@9999", 3).await.is_err());
    assert!(cli.get_market_liquidity(BTC, 101).await.is_err());

    let venue = PerpDexClients::Nado(cli);
    assert_eq!(venue.market(), NADO);
    assert_sane_book(
        &venue
            .get_orderbook(NVDA, InstrumentType::Perpetual, 5)
            .await
            .unwrap(),
        NVDA,
        5,
    );
    assert_eq!(
        venue.get_public_connect_msg(&bbo()).await.unwrap(),
        "wss://direct-gateway.prod.nado-backend.xyz/v1/subscribe"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "streams from live Nado and Aster for 30 seconds"]
async fn nado_live_streams() {
    let nado = PerpDexClients::Nado(NadoCli::default());
    let cases = vec![
        Case::new(1, nado.clone(), WsChannel::Lob(None), BTC, ""),
        Case::new(2, nado.clone(), bbo(), BTC, ""),
        Case::new(3, nado.clone(), WsChannel::Trades(None), BTC, ""),
        Case::new(
            4,
            nado.clone(),
            WsChannel::Lob(Some(LobParam::Incremental {
                depth: None,
                frequency: None,
            })),
            NVDA,
            "",
        ),
        Case::new(5, nado.clone(), bbo(), NVDA, ""),
        Case::new(
            6,
            nado,
            WsChannel::Trades(Some(TradesParam::AllTrades)),
            NVDA,
            "",
        ),
        Case::new(
            7,
            PerpDexClients::Aster(AsterCli::default()),
            bbo(),
            "NVDA_USDT_PERP",
            "",
        ),
    ];

    let snapshot = tokio::spawn(async {
        tokio::time::sleep(RUN_FOR / 2 - Duration::from_secs(3)).await;
        NadoCli::default()
            .get_market_liquidity(BTC, 0)
            .await
            .unwrap()
    });
    let run = live(cases, RUN_FOR).await;
    let snapshot = snapshot.await.unwrap();
    eprintln!("{}", run.summary());

    assert_clean(&run);
    assert_eq!(run.connects.len(), 7, "{:?}", run.connects);
    assert!(
        run.connects.values().all(|n| *n == 1),
        "reconnected: {:?}",
        run.connects
    );
    // Nado sends a BBO only when it changes, so a quiet stock can send none in half a minute.
    for id in [1, 2, 4, 7] {
        assert!(!run.lobs(id).is_empty(), "no book events on task {id}");
    }
    assert!(!run.trades(3).is_empty(), "no BTC trades");

    for (id, inst) in [(1, BTC), (4, NVDA)] {
        let book = run.lobs(id);
        for lob in book {
            assert!(
                matches!(
                    lob.event,
                    LobEventKind::Incremental | LobEventKind::Heartbeat
                ) && lob.market == NADO
                    && lob.inst == inst
                    && lob.timestamp > 1_700_000_000_000_000,
                "{lob:?}"
            );
            assert_sides_sorted(lob);
            let seq = lob.seq.as_ref().unwrap();
            assert!(seq.prev <= seq.first && seq.first <= seq.last, "{seq:?}");
            assert_eq!(lob.timestamp, seq.last.unwrap() / 1_000);
            for level in lob.bids.iter().chain(&lob.asks) {
                assert!(level.price > 0.0);
                assert_eq!(
                    matches!(level.action, LobLevelAction::Delete),
                    level.size == 0.0
                );
            }
        }
        assert_prev_chain(book);
    }

    // The BTC diffs splice onto the REST snapshot into an uncrossed book at the BBO.
    let book = run.lobs(1);
    assert!(
        book.first().unwrap().seq.as_ref().unwrap().last.unwrap() <= snapshot.timestamp,
        "the stream started after the snapshot"
    );
    let side = |levels: &[[String; 2]]| -> BTreeMap<u64, f64> {
        levels
            .iter()
            .map(|[p, s]| (nado_x18_to_f64(p).to_bits(), nado_x18_to_f64(s)))
            .collect()
    };
    let (mut bids, mut asks) = (side(&snapshot.bids), side(&snapshot.asks));
    let mut applied = 0;
    for lob in book
        .iter()
        .filter(|l| l.seq.as_ref().unwrap().last.unwrap() > snapshot.timestamp)
    {
        for (levels, book_side) in [(&lob.bids, &mut bids), (&lob.asks, &mut asks)] {
            for level in levels {
                match level.action {
                    LobLevelAction::Delete => book_side.remove(&level.price.to_bits()),
                    LobLevelAction::Upsert => book_side.insert(level.price.to_bits(), level.size),
                };
            }
        }
        let best_bid = f64::from_bits(*bids.keys().next_back().unwrap());
        let best_ask = f64::from_bits(*asks.keys().next().unwrap());
        assert!(
            best_bid < best_ask,
            "spliced book crossed: {best_bid} {best_ask}"
        );
        applied += 1;
    }
    assert!(applied > 0);
    let spliced_mid = (f64::from_bits(*bids.keys().next_back().unwrap())
        + f64::from_bits(*asks.keys().next().unwrap()))
        / 2.0;
    let bbo_mid = mid(run.lobs(2).last().unwrap());
    eprintln!("BTC spliced book mid {spliced_mid}, bbo mid {bbo_mid}, {applied} diffs applied");
    assert!((spliced_mid / bbo_mid - 1.0).abs() < 0.001);

    for (id, market, inst) in [
        (2, NADO, BTC),
        (5, NADO, NVDA),
        (7, ASTER, "NVDA_USDT_PERP"),
    ] {
        let lobs = run.lobs(id);
        lobs.iter().for_each(assert_bbo);
        assert!(
            lobs.iter().all(|l| l.market == market
                && l.inst == inst
                && l.timestamp > 1_700_000_000_000_000),
            "task {id}"
        );
        assert!(lobs.windows(2).all(|w| w[0].timestamp <= w[1].timestamp));
    }
    if let Some(nado_nvda) = run.lobs(5).last().map(mid) {
        let aster_nvda = mid(run.lobs(7).last().unwrap());
        eprintln!("NVDA mids (Nado, Aster): {nado_nvda}, {aster_nvda}");
        assert!((nado_nvda / aster_nvda - 1.0).abs() < 0.02);
    }

    for (id, inst, bbo_id) in [(3, BTC, 2), (6, NVDA, 5)] {
        let Some(reference) = run.lobs(bbo_id).last().map(mid) else {
            continue;
        };
        let trades = run.trades(id);
        for trade in trades {
            assert!(
                trade.market == NADO
                    && trade.inst == inst
                    && matches!(trade.side, OrderSide::BUY | OrderSide::SELL)
                    && trade.size > 0.0
                    && trade.trade_id == 0
                    && trade.timestamp > 1_700_000_000_000_000,
                "{trade:?}"
            );
            assert!((trade.price / reference - 1.0).abs() < 0.02, "{trade:?}");
        }
        assert!(trades.windows(2).all(|w| w[0].timestamp <= w[1].timestamp));
    }
}
