//! Replays frames captured from Pacifica through the infra runtime.

mod common;

use std::collections::HashSet;

use common::*;
use extrema_infra::prelude::*;
use perp_dex::prelude::*;
use serde_json::Value;

fn cases() -> Vec<Case> {
    let pacifica = || PerpDexClients::Pacifica(PacificaCli::default());

    vec![
        Case::new(
            1,
            pacifica(),
            WsChannel::Lob(None),
            "BTC_USDC_PERP",
            "pacifica_book",
        ),
        Case::new(
            2,
            pacifica(),
            WsChannel::Lob(Some(LobParam::Snapshot {
                depth: Some(10),
                frequency: Some(LobFrequency::Ms250),
            })),
            "BTC_USDC_PERP",
            "pacifica_book",
        ),
        Case::new(3, pacifica(), bbo(), "NVDA_USDC_PERP", "pacifica_bbo"),
        Case::new(4, pacifica(), bbo(), "SOL_USDC", "pacifica_spot_bbo"),
        Case::new(
            5,
            pacifica(),
            WsChannel::Trades(None),
            "SOL_USDC_PERP",
            "pacifica_trade",
        ),
        Case::new(
            6,
            pacifica(),
            WsChannel::Lob(None),
            "NOPE_USDC_PERP",
            "pacifica_bad",
        ),
    ]
}

fn data_frames(fixture: &str, channel: &str) -> Vec<Value> {
    parsed_frames(fixture)
        .into_iter()
        .filter(|f| f["channel"] == channel)
        .collect()
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

    // Book: every frame a full snapshot, `li` never going back.
    let frames = data_frames("pacifica_book", "book");
    let book = out.lobs(1);
    assert_eq!(book.len(), frames.len());
    for (lob, frame) in book.iter().zip(&frames) {
        assert!(matches!(lob.event, LobEventKind::Snapshot));
        assert_eq!(
            (&lob.market, lob.inst.as_str()),
            (&PACIFICA, "BTC_USDC_PERP")
        );
        assert_eq!((lob.bids.len(), lob.asks.len()), (5, 5));
        assert_book_side_order(lob);
        assert!(
            lob.bids
                .iter()
                .chain(&lob.asks)
                .all(|l| l.order_count.is_some_and(|n| n > 0))
        );
        assert_eq!(lob.seq.as_ref().unwrap().last, frame["data"]["li"].as_u64());
        assert_eq!(lob.timestamp, frame["data"]["t"].as_u64().unwrap() * 1_000);
    }
    assert_last_rising(book, false);
    let distinct: HashSet<_> = book.iter().map(|l| l.seq.as_ref().unwrap().last).collect();
    assert!(
        distinct.len() > 1 && distinct.len() < book.len(),
        "{distinct:?}"
    );
    assert_eq!(out.lobs(2).len(), book.len());

    // Stock and spot BBO: `li` rises with every top-of-book change.
    for (task, fixture, inst) in [
        (3, "pacifica_bbo", "NVDA_USDC_PERP"),
        (4, "pacifica_spot_bbo", "SOL_USDC"),
    ] {
        let bbo = out.lobs(task);
        assert_eq!(bbo.len(), data_frames(fixture, "bbo").len(), "task {task}");
        bbo.iter().for_each(assert_bbo);
        assert!(
            bbo.iter()
                .all(|l| l.market == PACIFICA && l.inst == inst && l.timestamp > 0)
        );
        assert_last_rising(bbo, true);
    }

    // Trades: one event per fill, taker side, venue history id.
    let fills: Vec<Value> = data_frames("pacifica_trade", "trades")
        .into_iter()
        .flat_map(|f| f["data"].as_array().unwrap().clone())
        .collect();
    let trades = out.trades(5);
    assert_eq!(trades.len(), fills.len());
    for (trade, fill) in trades.iter().zip(&fills) {
        assert_eq!(
            (&trade.market, trade.inst.as_str()),
            (&PACIFICA, "SOL_USDC_PERP")
        );
        assert_eq!(Some(trade.trade_id), fill["h"].as_u64());
        let taker_buys = matches!(fill["d"].as_str(), Some("open_long" | "close_short"));
        assert_eq!(
            trade.side,
            if taker_buys {
                OrderSide::BUY
            } else {
                OrderSide::SELL
            }
        );
        assert!(trade.price > 0.0 && trade.size > 0.0);
    }
    assert!(trades.iter().any(|t| t.side == OrderSide::BUY));
    assert!(trades.iter().any(|t| t.side == OrderSide::SELL));
    assert_eq!(
        trades
            .iter()
            .map(|t| t.trade_id)
            .collect::<HashSet<_>>()
            .len(),
        trades.len()
    );

    // A rejected subscription produces no events, only a warning.
    assert!(out.lobs(6).is_empty());
    assert!(
        out.logs.contains("Pacifica WS error: Symbol not found"),
        "{}",
        out.logs
    );
}
