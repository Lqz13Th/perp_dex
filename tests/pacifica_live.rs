//! Pacifica against the live venue: `cargo test --test pacifica_live -- --ignored`.

mod common;

use std::{collections::HashSet, sync::Arc, time::Duration};

use common::*;
use extrema_infra::{arch::market_assets::api_data::utils_data::InstrumentInfo, prelude::*};
use perp_dex::prelude::*;
use reqwest::Client;

const STOCK: &str = "NVDA";
const RUN_FOR: Duration = Duration::from_secs(30);
const KEEPALIVE_RUN_FOR: Duration = Duration::from_secs(150);

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
        assert!(info.min_notional.is_some() && info.max_leverage.is_some());
    }
}

#[tokio::test]
#[ignore = "hits the live Pacifica API"]
async fn pacifica_public_rest() {
    let cli = PacificaCli::new(Arc::new(Client::new()));
    let stock = format!("{STOCK}_{PACIFICA_QUOTE}_PERP");

    let markets = cli.get_market_info().await.unwrap();
    for market in &markets {
        assert_eq!(
            cli_to_pacifica_symbol(&market.inst()).unwrap(),
            market.symbol
        );
        assert_eq!(pacifica_inst_type(&market.symbol), market.inst_type());
        match market.inst_type() {
            InstrumentType::Perpetual => assert_eq!(market.base_asset, market.symbol),
            InstrumentType::Spot => assert_eq!(
                market.symbol,
                format!("{}-{PACIFICA_QUOTE}", market.base_asset),
                "spot pairs share the perp quote"
            ),
            other => panic!("{} is {other:?}", market.symbol),
        }
    }
    let symbols: HashSet<&str> = markets.iter().map(|m| m.symbol.as_str()).collect();
    assert!(symbols.contains(STOCK) && symbols.contains("TSLA") && symbols.contains("BTC"));

    let perps = cli
        .get_instrument_info(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert_sane_instruments(&perps);
    for info in &perps {
        assert_eq!(
            cli_to_pacifica_symbol(&info.inst).ok().as_deref(),
            info.inst_code.as_deref()
        );
        assert_eq!(
            pacifica_symbol_to_cli(info.inst_code.as_deref().unwrap()),
            info.inst
        );
    }
    let nvda = perps.iter().find(|i| i.inst == stock).unwrap();
    assert_eq!(nvda.state, InstrumentStatus::Live);
    assert_eq!(nvda.min_notional, Some(10.0));

    let spots = cli.get_instrument_info(InstrumentType::Spot).await.unwrap();
    assert_sane_instruments(&spots);
    assert!(spots.iter().all(|i| i.inst_type == InstrumentType::Spot));
    assert!(spots.iter().any(|i| i.inst == "SOL_USDC"));
    assert_eq!(perps.len() + spots.len(), markets.len());
    assert!(
        cli.get_instrument_info(InstrumentType::Futures)
            .await
            .unwrap()
            .is_empty()
    );

    let live = cli
        .get_live_instruments(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert_eq!(
        live.into_iter().collect::<HashSet<_>>(),
        perps.iter().map(|i| i.inst.clone()).collect()
    );

    let prices = cli.get_prices().await.unwrap();
    assert_eq!(
        prices
            .iter()
            .map(|p| p.symbol.as_str())
            .collect::<HashSet<_>>(),
        symbols
    );

    let insts = vec![stock.clone(), "BTC_USDC_PERP".to_string()];
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
    let spot_tickers = cli
        .get_tickers(None, Some(InstrumentType::Spot))
        .await
        .unwrap();
    assert_eq!(spot_tickers.len(), spots.len());
    assert!(
        spot_tickers
            .iter()
            .all(|t| t.inst_type == InstrumentType::Spot)
    );

    let marks = cli
        .get_mark_prices(Some(&insts), Some(InstrumentType::Perpetual))
        .await
        .unwrap();
    assert_eq!(marks.len(), 2);
    for mark in &marks {
        let mid = tickers.iter().find(|t| t.inst == mark.inst).unwrap().price;
        assert!(
            (mark.mark_price / mid - 1.0).abs() < 0.05,
            "{mark:?} vs {mid}"
        );
    }
    assert_eq!(
        cli.get_mark_prices(None, None).await.unwrap().len(),
        markets.len()
    );

    let book = cli
        .get_orderbook(&stock, InstrumentType::Perpetual, 5)
        .await
        .unwrap();
    assert_sane_book(&book, &stock, 5);
    let deep = cli
        .get_orderbook("BTC_USDC_PERP", InstrumentType::Perpetual, 0)
        .await
        .unwrap();
    assert_sane_book(&deep, "BTC_USDC_PERP", 10);
    assert!(deep.bids.len() > 5);
    let spot = cli
        .get_orderbook("SOL_USDC", InstrumentType::Spot, 3)
        .await
        .unwrap();
    assert_sane_book(&spot, "SOL_USDC", 3);

    for (inst, inst_type, depth) in [
        ("BTC_USDC_PERP", InstrumentType::Perpetual, 11),
        ("BTC_USDC_PERP", InstrumentType::Spot, 5),
        ("NVDA_USDT_PERP", InstrumentType::Perpetual, 5),
        ("btc_USDC_PERP", InstrumentType::Perpetual, 5),
    ] {
        assert!(
            cli.get_orderbook(inst, inst_type, depth).await.is_err(),
            "{inst}"
        );
    }
    let err = cli
        .get_orderbook("NOPE_USDC_PERP", InstrumentType::Perpetual, 5)
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("Pacifica REST error") && err.to_string().contains("NOPE"),
        "{err}"
    );
}

#[tokio::test]
#[ignore = "hits the live Pacifica API"]
async fn pacifica_dispatches_through_perp_dex_clients() {
    let venue = PerpDexClients::Pacifica(PacificaCli::default());
    let stock = format!("{STOCK}_{PACIFICA_QUOTE}_PERP");

    assert_eq!(venue.market(), PACIFICA);
    assert!(
        venue
            .get_instrument_info(InstrumentType::Perpetual)
            .await
            .unwrap()
            .iter()
            .any(|i| i.inst == stock)
    );
    let book = venue
        .get_orderbook(&stock, InstrumentType::Perpetual, 5)
        .await
        .unwrap();
    assert_sane_book(&book, &stock, 5);
    assert_eq!(
        venue.get_public_connect_msg(&bbo()).await.unwrap(),
        "wss://ws.pacifica.fi/ws"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "streams from Pacifica and Aster for 30 seconds"]
async fn pacifica_live_streams_decode_cleanly() {
    let pacifica = PerpDexClients::Pacifica(PacificaCli::default());
    let aster = PerpDexClients::Aster(AsterCli::default());
    let stock = format!("{STOCK}_{PACIFICA_QUOTE}_PERP");
    let cases = vec![
        Case::new(
            1,
            pacifica.clone(),
            WsChannel::Lob(None),
            "BTC_USDC_PERP",
            "",
        ),
        Case::new(2, pacifica.clone(), bbo(), "BTC_USDC_PERP", ""),
        Case::new(
            3,
            pacifica.clone(),
            WsChannel::Trades(None),
            "BTC_USDC_PERP",
            "",
        ),
        Case::new(4, pacifica.clone(), WsChannel::Lob(None), &stock, ""),
        Case::new(5, pacifica.clone(), bbo(), &stock, ""),
        Case::new(6, pacifica, WsChannel::Trades(None), &stock, ""),
        Case::new(7, aster, bbo(), &format!("{STOCK}_USDT_PERP"), ""),
    ];

    let run = live(cases, RUN_FOR).await;
    eprintln!("{}", run.summary());

    assert_clean(&run);
    assert!(
        run.connects.values().all(|n| *n == 1),
        "reconnected: {:?}",
        run.connects
    );

    for (book_id, bbo_id, inst) in [(1, 2, "BTC_USDC_PERP"), (4, 5, stock.as_str())] {
        let book = run.lobs(book_id);
        assert!(book.len() > 10, "task {book_id}: {} books", book.len());
        for lob in book {
            assert!(matches!(lob.event, LobEventKind::Snapshot));
            assert!(lob.bids.len() <= 10 && lob.asks.len() <= 10);
            assert!(!lob.bids.is_empty() && !lob.asks.is_empty());
            assert_book_side_order(lob);
            assert!(
                lob.market == PACIFICA && lob.inst == inst && lob.timestamp > 1_700_000_000_000_000
            );
        }
        assert_last_rising(book, false);

        let bbo = run.lobs(bbo_id);
        assert!(!bbo.is_empty(), "task {bbo_id}: no bbo");
        bbo.iter().for_each(assert_bbo);
        assert!(bbo.iter().all(|l| l.market == PACIFICA
            && l.inst == inst
            && l.timestamp > 1_700_000_000_000_000));
        assert_last_rising(bbo, true);

        let last_book = book.last().unwrap();
        let last_bbo = bbo.last().unwrap();
        assert!((mid(last_book) / mid(last_bbo) - 1.0).abs() < 0.005);
    }

    for task in [3, 6] {
        let trades = run.trades(task);
        eprintln!("task {task}: {} trades", trades.len());
        for trade in trades {
            assert!(
                trade.market == PACIFICA
                    && matches!(trade.side, OrderSide::BUY | OrderSide::SELL)
                    && trade.price > 0.0
                    && trade.size > 0.0
                    && trade.timestamp > 1_700_000_000_000_000
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

    let pacifica_mid = mid(run.lobs(5).last().unwrap());
    let aster_bbo = run.lobs(7);
    aster_bbo.iter().for_each(assert_bbo);
    let aster_mid = mid(aster_bbo.last().unwrap());
    eprintln!("{STOCK} mids (Pacifica, Aster): {pacifica_mid} {aster_mid}");
    assert!(
        (pacifica_mid / aster_mid - 1.0).abs() < 0.02,
        "{STOCK} mids disagree: {pacifica_mid} vs {aster_mid}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "streams from Pacifica for 150 seconds"]
async fn pacifica_keepalive_holds_busy_streams_past_the_idle_limit() {
    let pacifica = || PerpDexClients::Pacifica(PacificaCli::default());
    let cases = vec![
        Case::new(1, pacifica(), WsChannel::Lob(None), "BTC_USDC_PERP", ""),
        Case::new(2, pacifica(), WsChannel::Lob(None), "BTC_USDC_PERP", "")
            .with_keepalive(pacifica_keepalive()),
        Case::new(3, pacifica(), bbo(), "BTC_USDC_PERP", "").with_keepalive(pacifica_keepalive()),
        Case::new(4, pacifica(), WsChannel::Trades(None), "PONS_USDC_PERP", ""),
    ];

    let run = live(cases, KEEPALIVE_RUN_FOR).await;
    eprintln!("{}", run.summary());

    assert!(!run.logs.contains("Failed to deserialize"), "{}", run.logs);
    assert!(run.lobs(2).len() > 300 && !run.lobs(3).is_empty());
    assert_eq!((run.connects[&2], run.connects[&3]), (1, 1));
    assert_eq!(
        run.connects[&4], 1,
        "a quiet stream lives on infra's own ping"
    );
    assert!(
        run.connects[&1] > 1,
        "Pacifica no longer drops busy streams without a client frame: {:?}",
        run.connects
    );
}
