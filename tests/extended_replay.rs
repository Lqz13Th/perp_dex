//! Replays frames captured from Extended through the infra runtime.

mod common;

use std::collections::HashSet;

use common::*;
use extrema_infra::prelude::*;
use perp_dex::prelude::*;

const BTC: &str = "BTC_USD_PERP";
const NVDA: &str = "NVDA_24_5_USD_PERP";

fn cases() -> Vec<Case> {
    let extended = || PerpDexClients::Extended(ExtendedCli::default());

    vec![
        Case::new(1, extended(), WsChannel::Lob(None), BTC, "extended_book"),
        Case::new(2, extended(), bbo(), BTC, "extended_bbo"),
        Case::new(
            3,
            extended(),
            WsChannel::Trades(None),
            BTC,
            "extended_trade",
        ),
        Case::new(
            4,
            extended(),
            WsChannel::Lob(Some(LobParam::Incremental {
                depth: None,
                frequency: Some(LobFrequency::Ms100),
            })),
            NVDA,
            "extended_nvda_book",
        ),
        Case::new(5, extended(), bbo(), NVDA, "extended_nvda_bbo"),
        Case::new(
            6,
            extended(),
            WsChannel::Other("prices/mark".into()),
            BTC,
            "extended_mark",
        ),
        Case::new(
            7,
            extended(),
            WsChannel::Lob(None),
            "NOPE_USD_PERP",
            "extended_bad",
        ),
    ]
}

fn book_frames(fixture: &str) -> usize {
    count_frames(fixture, |f| {
        matches!(f["type"].as_str(), Some("SNAPSHOT" | "DELTA"))
    })
}

fn assert_book(book: &[WsLob], fixture: &str, inst: &str) {
    let frames = parsed_frames(fixture);
    assert_eq!(book.len(), book_frames(fixture));
    assert!(matches!(book[0].event, LobEventKind::Snapshot));
    assert_eq!(book[0].seq.as_ref().unwrap().last, Some(1));
    assert_eq!((book[0].bids.len(), book[0].asks.len()), (5, 5));
    assert_book_side_order(&book[0]);
    assert!(
        book[1..]
            .iter()
            .all(|l| matches!(l.event, LobEventKind::Incremental | LobEventKind::Heartbeat))
    );
    assert_last_contiguous(book);
    for (lob, frame) in book.iter().zip(&frames) {
        assert!(lob.market == EXTENDED && lob.inst == inst, "{lob:?}");
        assert_eq!(lob.timestamp, frame["ts"].as_u64().unwrap() * 1_000);
        assert_eq!(
            (lob.bids.len(), lob.asks.len()),
            (
                frame["data"]["b"].as_array().unwrap().len(),
                frame["data"]["a"].as_array().unwrap().len()
            )
        );
    }
}

fn assert_bbo_stream(bbo: &[WsLob], fixture: &str, inst: &str) {
    assert_eq!(bbo.len(), book_frames(fixture));
    bbo.iter().for_each(assert_bbo);
    assert_last_contiguous(bbo);
    assert!(
        bbo.iter()
            .all(|l| l.market == EXTENDED && l.inst == inst && l.timestamp > 0)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn captured_frames_decode_end_to_end_through_the_runtime() {
    let out = replay(cases()).await;
    eprintln!("{}", out.summary());

    assert!(
        out.unmatched.is_empty(),
        "stream URLs differ from live: {:?}",
        out.unmatched
    );
    assert!(!out.logs.contains("Failed to deserialize"), "{}", out.logs);
    assert_eq!(out.connects.len(), 7);
    assert!(out.connects.values().all(|n| *n == 1), "{:?}", out.connects);

    // Full books: a snapshot, then deltas numbered one by one from 1.
    assert_book(out.lobs(1), "extended_book", BTC);
    assert_book(out.lobs(4), "extended_nvda_book", NVDA);
    assert!(
        out.lobs(1)[1..]
            .iter()
            .flat_map(|l| l.bids.iter().chain(&l.asks))
            .any(|level| matches!(level.action, LobLevelAction::Delete))
    );

    // Best bid and ask, one snapshot per frame.
    assert_bbo_stream(out.lobs(2), "extended_bbo", BTC);
    assert_bbo_stream(out.lobs(5), "extended_nvda_bbo", NVDA);

    // Trades: live prints only, never the history replayed on connect.
    let frames = parsed_frames("extended_trade");
    let history: HashSet<u64> = frames[0]["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["i"].as_u64().unwrap())
        .collect();
    assert_eq!(frames[0]["seq"], 1);
    assert!(!history.is_empty());
    let expected: usize = frames[1..]
        .iter()
        .map(|f| f["data"].as_array().unwrap().len())
        .sum();
    let trades = out.trades(3);
    assert_eq!(trades.len(), expected);
    assert!(
        trades
            .iter()
            .all(|t| t.market == EXTENDED && t.inst == BTC && t.price > 0.0 && t.size > 0.0)
    );
    assert!(
        trades
            .iter()
            .all(|t| matches!(t.side, OrderSide::BUY | OrderSide::SELL))
    );
    let ids: HashSet<u64> = trades.iter().map(|t| t.trade_id).collect();
    assert_eq!(ids.len(), trades.len());
    assert!(ids.is_disjoint(&history));

    // Raw streams reach on_ws_other; an unknown market streams nothing.
    assert!(out.others.contains(&6) && out.others.len() == 1);
    assert!(out.lobs(7).is_empty());
}
