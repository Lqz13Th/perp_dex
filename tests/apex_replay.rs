//! Replays frames captured from ApeX through the infra runtime.

mod common;

use std::collections::HashSet;

use common::*;
use extrema_infra::prelude::*;
use perp_dex::prelude::*;

fn book25() -> WsChannel {
    WsChannel::Lob(Some(LobParam::Incremental {
        depth: Some(25),
        frequency: None,
    }))
}

fn cases() -> Vec<Case> {
    let apex = || PerpDexClients::Apex(ApexCli::default());

    vec![
        Case::new(
            1,
            apex(),
            WsChannel::Lob(None),
            "BTC_USDT_PERP",
            "apex_book",
        ),
        Case::new(2, apex(), book25(), "NVDA_USDT_PERP", "apex_book25"),
        Case::new(
            3,
            apex(),
            WsChannel::Trades(None),
            "BTC_USDT_PERP",
            "apex_trade",
        ),
        Case::new(
            4,
            apex(),
            WsChannel::Other("instrumentInfo.H".into()),
            "BTC_USDT_PERP",
            "apex_other",
        ),
        Case::new(5, apex(), book25(), "NOPE_USDT_PERP", "apex_bad"),
    ]
}

fn assert_book(lobs: &[WsLob], fixture: &str, inst: &str) {
    assert_eq!(
        lobs.len(),
        count_frames(fixture, |f| f.get("topic").is_some())
    );
    assert!(matches!(lobs[0].event, LobEventKind::Snapshot));
    assert_eq!((lobs[0].bids.len(), lobs[0].asks.len()), (5, 5));
    assert_book_side_order(&lobs[0]);
    assert!(
        lobs[1..]
            .iter()
            .all(|l| matches!(l.event, LobEventKind::Incremental | LobEventKind::Heartbeat))
    );
    assert!(
        lobs[1..]
            .iter()
            .flat_map(|l| l.bids.iter().chain(&l.asks))
            .any(|level| matches!(level.action, LobLevelAction::Delete))
    );
    assert_last_contiguous(lobs);
    assert!(lobs.iter().all(|l| l.market == APEX
        && l.inst == inst
        && l.timestamp > 1_700_000_000_000_000
        && l.seq.as_ref().is_some_and(|s| s.first == s.last)));
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

    // Books: snapshot re-sorted best first, then deltas whose update id rises by 1.
    assert_book(out.lobs(1), "apex_book", "BTC_USDT_PERP");
    assert_book(out.lobs(2), "apex_book25", "NVDA_USDT_PERP");

    // Trades: live fills only, never the 50-trade subscribe history.
    let expected: usize = parsed_frames("apex_trade")
        .iter()
        .filter(|f| f["type"] == "delta")
        .map(|f| f["data"].as_array().unwrap().len())
        .sum();
    let trades = out.trades(3);
    assert_eq!(trades.len(), expected);
    assert!(
        trades
            .iter()
            .all(|t| t.market == APEX && t.inst == "BTC_USDT_PERP" && t.price > 0.0)
    );
    assert!(
        trades
            .iter()
            .all(|t| matches!(t.side, OrderSide::BUY | OrderSide::SELL))
    );
    assert!(trades.windows(2).all(|w| w[0].timestamp <= w[1].timestamp));
    assert_eq!(
        trades
            .iter()
            .map(|t| t.trade_id)
            .collect::<HashSet<_>>()
            .len(),
        trades.len()
    );

    // Raw topics pass through untouched.
    assert!(out.others.contains(&4) && out.others.len() == 1);

    // A rejected subscription produces no events, only a warning.
    assert!(out.lobs(5).is_empty());
    assert!(out.logs.contains("ApeX WS error"), "{}", out.logs);
    assert!(out.logs.contains("handler not found"), "{}", out.logs);
}
