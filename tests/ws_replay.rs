//! Replays frames captured from Lighter, Aster and Arcus through the infra runtime.

mod common;

use std::collections::HashSet;

use common::*;
use extrema_infra::prelude::*;
use perp_dex::prelude::*;

fn cases() -> Vec<Case> {
    let lighter = || PerpDexClients::Lighter(LighterCli::default());
    let aster = || PerpDexClients::Aster(AsterCli::default());
    let arcus = || PerpDexClients::Arcus(ArcusCli::default());
    let lighter_rh = || {
        let mut cli = LighterCli::default();
        cli.set_venue(LighterVenue::Robinhood);
        PerpDexClients::Lighter(cli)
    };

    vec![
        Case::new(1, lighter(), WsChannel::Lob(None), "@1", "lighter_book"),
        Case::new(2, lighter(), bbo(), "@1", "lighter_ticker"),
        Case::new(3, lighter(), WsChannel::Trades(None), "@1", "lighter_trade"),
        Case::new(4, aster(), bbo(), "BTC_USDT_PERP", "aster_bbo"),
        Case::new(
            5,
            aster(),
            WsChannel::Lob(Some(LobParam::Incremental {
                depth: None,
                frequency: Some(LobFrequency::Ms100),
            })),
            "BTC_USDT_PERP",
            "aster_diff",
        ),
        Case::new(
            6,
            aster(),
            WsChannel::Lob(Some(LobParam::Snapshot {
                depth: Some(5),
                frequency: Some(LobFrequency::Ms100),
            })),
            "BTC_USDT_PERP",
            "aster_depth5",
        ),
        Case::new(
            7,
            aster(),
            WsChannel::Trades(Some(TradesParam::AllTrades)),
            "BTC_USDT_PERP",
            "aster_trade",
        ),
        Case::new(
            8,
            aster(),
            WsChannel::Trades(None),
            "BTC_USDT_PERP",
            "aster_agg",
        ),
        Case::new(
            9,
            arcus(),
            WsChannel::Lob(Some(LobParam::Incremental {
                depth: Some(5),
                frequency: None,
            })),
            "NVDA_USD_PERP",
            "arcus_book",
        ),
        Case::new(
            10,
            arcus(),
            WsChannel::Lob(Some(LobParam::Snapshot {
                depth: Some(5),
                frequency: None,
            })),
            "NVDA_USD_PERP",
            "arcus_snap",
        ),
        Case::new(11, arcus(), bbo(), "BTC_USD_PERP", "arcus_bbo"),
        Case::new(
            12,
            arcus(),
            WsChannel::Trades(None),
            "BTC_USD_PERP",
            "arcus_trade",
        ),
        Case::new(13, lighter(), WsChannel::Lob(None), "@9999", "lighter_bad"),
        Case::new(14, arcus(), bbo(), "NOPE_USD_PERP", "arcus_bad"),
        Case::new(
            15,
            lighter_rh(),
            WsChannel::Lob(None),
            "@26",
            "lighter_rh_book",
        ),
        Case::new(16, lighter_rh(), bbo(), "@26", "lighter_rh_ticker"),
        Case::new(
            17,
            lighter_rh(),
            WsChannel::Trades(None),
            "@0",
            "lighter_rh_trade",
        ),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn captured_frames_decode_end_to_end_through_the_runtime() {
    let out = replay(cases()).await;
    eprintln!("{}", out.summary());

    assert!(
        out.unmatched.is_empty(),
        "subscribe messages differ from live: {:?}",
        out.unmatched
    );
    assert!(!out.logs.contains("Failed to deserialize"), "{}", out.logs);
    assert!(out.others.is_empty(), "no task subscribed to a raw channel");

    // Lighter order book: snapshot, then updates chained by nonce.
    let book = out.lobs(1);
    assert_eq!(
        book.len(),
        count_frames("lighter_book", |f| f.get("order_book").is_some())
    );
    assert!(matches!(book[0].event, LobEventKind::Snapshot));
    assert_eq!((book[0].bids.len(), book[0].asks.len()), (5, 5));
    assert_book_side_order(&book[0]);
    assert!(
        book[1..]
            .iter()
            .all(|l| !matches!(l.event, LobEventKind::Snapshot | LobEventKind::Bbo))
    );
    assert_prev_chain(book);
    assert!(
        book.iter()
            .all(|l| l.market == LIGHTER && l.inst == "@1" && l.timestamp > 0)
    );

    // Lighter ticker.
    let ticker = out.lobs(2);
    assert_eq!(
        ticker.len(),
        count_frames("lighter_ticker", |f| f.get("ticker").is_some())
    );
    assert!(ticker.iter().all(|l| l.inst == "@1"));
    ticker.iter().for_each(assert_bbo);

    // Lighter trades: live prints only, never the subscribe history.
    let expected: usize = parsed_frames("lighter_trade")
        .iter()
        .filter(|f| f["type"] == "update/trade")
        .map(|f| {
            f["trades"].as_array().unwrap().len()
                + f["liquidation_trades"].as_array().unwrap().len()
        })
        .sum();
    let trades = out.trades(3);
    assert_eq!(trades.len(), expected);
    assert!(trades.iter().all(|t| t.market == LIGHTER && t.inst == "@1"));
    assert!(
        trades
            .iter()
            .all(|t| matches!(t.side, OrderSide::BUY | OrderSide::SELL))
    );
    assert_eq!(
        trades
            .iter()
            .map(|t| t.trade_id)
            .collect::<HashSet<_>>()
            .len(),
        trades.len()
    );

    // Aster book ticker, diff depth chained by pu, partial depth snapshots.
    let aster_bbo = out.lobs(4);
    assert_eq!(
        aster_bbo.len(),
        count_frames("aster_bbo", |f| f["e"] == "bookTicker")
    );
    assert!(
        aster_bbo
            .iter()
            .all(|l| l.market == ASTER && l.inst == "BTC_USDT_PERP")
    );

    let diff = out.lobs(5);
    assert_eq!(
        diff.len(),
        count_frames("aster_diff", |f| f["e"] == "depthUpdate")
    );
    assert!(
        diff.iter()
            .all(|l| matches!(l.event, LobEventKind::Incremental | LobEventKind::Heartbeat))
    );
    assert_prev_chain(diff);

    let depth5 = out.lobs(6);
    assert_eq!(
        depth5.len(),
        count_frames("aster_depth5", |f| f["e"] == "depthUpdate")
    );
    for lob in depth5 {
        assert!(matches!(lob.event, LobEventKind::Snapshot));
        assert!(lob.bids.len() <= 5 && lob.asks.len() <= 5);
        assert_book_side_order(lob);
    }

    assert_eq!(
        out.trades(7).len(),
        count_frames("aster_trade", |f| f["e"] == "trade")
    );
    assert_eq!(
        out.trades(8).len(),
        count_frames("aster_agg", |f| f["e"] == "aggTrade")
    );
    assert!(
        out.trades(7)
            .iter()
            .chain(out.trades(8))
            .all(|t| t.market == ASTER && t.inst == "BTC_USDT_PERP")
    );

    // Arcus updates: timed snapshot, untimed deltas contiguous after the first.
    let arcus_book = out.lobs(9);
    assert_eq!(
        arcus_book.len(),
        count_frames("arcus_book", |f| {
            f["channel"] == "l2OrderbookUpdates" && f.get("contents").is_some()
        })
    );
    assert!(matches!(arcus_book[0].event, LobEventKind::Snapshot));
    assert!(arcus_book[0].timestamp > 0);
    assert_book_side_order(&arcus_book[0]);
    assert!(arcus_book[1..].iter().all(|l| l.timestamp == 0));
    assert_last_contiguous(&arcus_book[1..]);

    let arcus_snap = out.lobs(10);
    assert_eq!(
        arcus_snap.len(),
        count_frames("arcus_snap", |f| f["channel"] == "l2Orderbook")
    );
    for lob in arcus_snap {
        assert!(matches!(lob.event, LobEventKind::Snapshot) && lob.timestamp > 0);
        assert_book_side_order(lob);
    }

    let arcus_bbo = out.lobs(11);
    assert_eq!(
        arcus_bbo.len(),
        count_frames("arcus_bbo", |f| f["channel"] == "bbo")
    );
    assert!(
        arcus_bbo
            .iter()
            .all(|l| l.market == ARCUS && l.inst == "BTC_USD_PERP" && l.timestamp > 0)
    );

    let expected: usize = parsed_frames("arcus_trade")
        .iter()
        .filter(|f| f["type"] == "channel_data")
        .map(|f| f["contents"].as_array().unwrap().len())
        .sum();
    assert_eq!(out.trades(12).len(), expected);
    assert!(
        out.trades(12)
            .iter()
            .all(|t| t.market == ARCUS && t.inst == "BTC_USD_PERP")
    );

    // Lighter on Robinhood Chain: same protocol, its own market.
    let rh_book = out.lobs(15);
    assert_eq!(
        rh_book.len(),
        count_frames("lighter_rh_book", |f| f.get("order_book").is_some())
    );
    assert!(matches!(rh_book[0].event, LobEventKind::Snapshot));
    assert_prev_chain(rh_book);
    assert!(
        rh_book
            .iter()
            .all(|l| l.market == LIGHTER_RH && l.inst == "@26")
    );
    let rh_ticker = out.lobs(16);
    assert_eq!(
        rh_ticker.len(),
        count_frames("lighter_rh_ticker", |f| f.get("ticker").is_some())
    );
    rh_ticker.iter().for_each(assert_bbo);
    assert!(rh_ticker.iter().all(|l| l.market == LIGHTER_RH));
    let expected: usize = parsed_frames("lighter_rh_trade")
        .iter()
        .filter(|f| f["type"] == "update/trade")
        .map(|f| {
            f["trades"].as_array().unwrap().len()
                + f["liquidation_trades"].as_array().unwrap().len()
        })
        .sum();
    assert_eq!(out.trades(17).len(), expected);
    assert!(
        out.trades(17)
            .iter()
            .all(|t| t.market == LIGHTER_RH && t.inst == "@0")
    );

    // Rejected subscriptions produce no events, only a warning.
    assert!(out.lobs(13).is_empty() && out.lobs(14).is_empty());
    assert!(out.logs.contains("Lighter WS error"), "{}", out.logs);
    assert!(out.logs.contains("Arcus WS error"), "{}", out.logs);
}
