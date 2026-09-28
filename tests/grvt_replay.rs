//! Replays frames captured from GRVT through the infra runtime.

mod common;

use common::*;
use extrema_infra::prelude::*;
use perp_dex::prelude::*;
use serde_json::Value;

fn cases() -> Vec<Case> {
    let grvt = || PerpDexClients::Grvt(GrvtCli::default());

    vec![
        Case::new(
            1,
            grvt(),
            WsChannel::Lob(None),
            "BTC_USDT_PERP",
            "grvt_book",
        ),
        Case::new(
            2,
            grvt(),
            WsChannel::Lob(Some(LobParam::Snapshot {
                depth: Some(10),
                frequency: None,
            })),
            "BTC_USDT_PERP",
            "grvt_snap",
        ),
        Case::new(3, grvt(), bbo(), "BTC_USDT_PERP", "grvt_bbo"),
        Case::new(
            4,
            grvt(),
            WsChannel::Trades(None),
            "ETH_USDT_PERP",
            "grvt_trade",
        ),
        Case::new(
            5,
            grvt(),
            WsChannel::Lob(None),
            "NOPE_USDT_PERP",
            "grvt_bad",
        ),
    ]
}

fn is_feed(frame: &Value) -> bool {
    frame.get("feed").is_some()
}

fn is_live_feed(frame: &Value) -> bool {
    is_feed(frame) && frame["sequence_number"] != "0"
}

fn first_sequence_number(fixture: &str) -> u64 {
    parsed_frames(fixture)[0]["result"]["first_sequence_number"][0]
        .as_str()
        .unwrap()
        .parse()
        .unwrap()
}

fn assert_grvt(lobs: &[WsLob], inst: &str) {
    assert!(
        lobs.iter()
            .all(|l| l.market == GRVT && l.inst == inst && l.timestamp > 1_700_000_000_000_000),
        "{lobs:?}"
    );
    assert!(
        lobs.windows(2).all(|w| w[0].timestamp <= w[1].timestamp),
        "timestamps go back"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn captured_grvt_frames_decode_end_to_end_through_the_runtime() {
    let out = replay(cases()).await;
    eprintln!("{}", out.summary());

    assert!(
        out.unmatched.is_empty(),
        "subscribe messages differ from live: {:?}",
        out.unmatched
    );
    assert!(!out.logs.contains("Failed to deserialize"), "{}", out.logs);
    assert!(out.others.is_empty(), "no task subscribed to a raw channel");
    assert!(out.logs.contains("GRVT WS subscribe reply"), "{}", out.logs);

    // Book deltas: an unsequenced snapshot, then deltas chained from the acked first sequence.
    let book = out.lobs(1);
    assert_eq!(book.len(), count_frames("grvt_book", is_feed));
    assert!(matches!(book[0].event, LobEventKind::Snapshot));
    assert_eq!((book[0].bids.len(), book[0].asks.len()), (5, 5));
    assert_book_side_order(&book[0]);
    assert_eq!(book[0].seq.as_ref().unwrap().last, None);
    assert!(
        book[1..]
            .iter()
            .all(|l| matches!(l.event, LobEventKind::Incremental | LobEventKind::Heartbeat))
    );
    assert_eq!(
        book[1].seq.as_ref().unwrap().last,
        Some(first_sequence_number("grvt_book"))
    );
    assert_prev_chain(&book[1..]);
    assert_last_contiguous(&book[1..]);
    assert!(
        book[1..]
            .iter()
            .flat_map(|l| l.bids.iter().chain(&l.asks))
            .any(|level| matches!(level.action, LobLevelAction::Delete) && level.size == 0.0)
    );
    assert!(
        book.iter()
            .flat_map(|l| l.bids.iter().chain(&l.asks))
            .all(|level| level.order_count.is_some())
    );
    assert_grvt(book, "BTC_USDT_PERP");

    // Book snapshots: every frame a full book, sequenced after the first.
    let snap = out.lobs(2);
    assert_eq!(snap.len(), count_frames("grvt_snap", is_feed));
    for lob in snap {
        assert!(matches!(lob.event, LobEventKind::Snapshot));
        assert!(lob.bids.len() <= 10 && lob.asks.len() <= 10);
        assert_book_side_order(lob);
    }
    assert_eq!(
        snap[1].seq.as_ref().unwrap().last,
        Some(first_sequence_number("grvt_snap"))
    );
    assert_prev_chain(&snap[1..]);
    assert_grvt(snap, "BTC_USDT_PERP");

    // Mini ticker as BBO.
    let bbo = out.lobs(3);
    assert_eq!(bbo.len(), count_frames("grvt_bbo", is_feed));
    bbo.iter().for_each(assert_bbo);
    assert_prev_chain(&bbo[1..]);
    assert_grvt(bbo, "BTC_USDT_PERP");

    // Trades: live fills only, never the replayed history.
    let frames = parsed_frames("grvt_trade");
    let expected: Vec<u64> = frames
        .iter()
        .filter(|f| is_live_feed(f))
        .map(|f| grvt_trade_id_to_u64(f["feed"]["trade_id"].as_str().unwrap()).unwrap())
        .collect();
    assert!(frames.iter().any(|f| is_feed(f) && !is_live_feed(f)));
    let trades = out.trades(4);
    assert_eq!(
        trades.iter().map(|t| t.trade_id).collect::<Vec<_>>(),
        expected
    );
    assert!(
        trades.iter().all(|t| t.market == GRVT
            && t.inst == "ETH_USDT_PERP"
            && t.price > 0.0
            && t.size > 0.0)
    );
    assert!(trades.iter().any(|t| t.side == OrderSide::BUY));
    assert!(trades.iter().any(|t| t.side == OrderSide::SELL));
    assert!(trades.windows(2).all(|w| w[0].trade_id < w[1].trade_id));

    // A rejected subscription produces no events, only a warning.
    assert!(out.lobs(5).is_empty());
    assert!(out.logs.contains("GRVT WS error"), "{}", out.logs);
    assert!(out.logs.contains("Instrument is invalid"), "{}", out.logs);
}
