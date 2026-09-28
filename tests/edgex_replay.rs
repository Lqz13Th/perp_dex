//! Replays frames captured from edgeX through the infra runtime.

mod common;

use std::collections::HashSet;

use common::*;
use extrema_infra::prelude::*;
use perp_dex::prelude::*;
use serde_json::Value;

fn quote_events(fixture: &str) -> Vec<Value> {
    parsed_frames(fixture)
        .into_iter()
        .filter(|f| f["type"] == "quote-event")
        .collect()
}

fn entries(frames: &[Value]) -> usize {
    frames
        .iter()
        .map(|f| f["content"]["data"].as_array().unwrap().len())
        .sum()
}

fn cases() -> Vec<Case> {
    let edgex = || PerpDexClients::Edgex(EdgexCli::default());
    let depth15 = WsChannel::Lob(Some(LobParam::Incremental {
        depth: Some(15),
        frequency: None,
    }));

    vec![
        Case::new(1, edgex(), depth15.clone(), "@30000001", "edgex_book"),
        Case::new(
            2,
            edgex(),
            WsChannel::Lob(None),
            "@30000020",
            "edgex_book200",
        ),
        Case::new(3, edgex(), bbo(), "", "edgex_bbo"),
        Case::new(
            4,
            edgex(),
            WsChannel::Trades(None),
            "@30000001",
            "edgex_trade",
        ),
        Case::new(
            5,
            edgex(),
            WsChannel::Other("ticker".into()),
            "@30000020",
            "edgex_ticker",
        ),
        Case::new(6, edgex(), depth15, "@99999999", "edgex_bad"),
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

    // Books: an untimed snapshot, then deltas chained by version.
    for (task, fixture, inst) in [
        (1, "edgex_book", "@30000001"),
        (2, "edgex_book200", "@30000020"),
    ] {
        let book = out.lobs(task);
        assert_eq!(book.len(), quote_events(fixture).len(), "{fixture}");
        assert!(matches!(book[0].event, LobEventKind::Snapshot));
        assert_eq!((book[0].bids.len(), book[0].asks.len()), (5, 5));
        assert_book_side_order(&book[0]);
        assert!(
            book[1..]
                .iter()
                .all(|l| !matches!(l.event, LobEventKind::Snapshot | LobEventKind::Bbo))
        );
        assert!(book[1..].iter().any(|l| {
            l.bids
                .iter()
                .chain(&l.asks)
                .any(|level| matches!(level.action, LobLevelAction::Delete))
        }));
        assert_prev_chain(book);
        assert!(
            book.iter()
                .all(|l| l.market == EDGEX && l.inst == inst && l.timestamp == 0)
        );
    }

    // All-market BBO: every entry of every frame, snapshot included.
    let bbo = out.lobs(3);
    assert_eq!(bbo.len(), entries(&quote_events("edgex_bbo")));
    assert!(
        bbo.iter()
            .all(|l| l.market == EDGEX && l.timestamp > 1_700_000_000_000_000)
    );
    let insts: HashSet<&str> = bbo.iter().map(|l| l.inst.as_str()).collect();
    assert!(insts.contains("@30000001") && insts.contains("@30000020"));
    for lob in bbo {
        if lob.inst == "@30000158" {
            assert!(lob.bids.is_empty() && lob.asks.is_empty(), "{lob:?}");
        } else {
            assert_bbo(lob);
        }
    }

    // Trades: live fills only, never the subscribe history.
    let live_fills: Vec<Value> = quote_events("edgex_trade")
        .into_iter()
        .filter(|f| f["content"]["dataType"] == "changed")
        .collect();
    let trades = out.trades(4);
    assert!(!live_fills.is_empty());
    assert_eq!(trades.len(), entries(&live_fills));
    assert!(trades.iter().all(|t| t.market == EDGEX
        && t.inst == "@30000001"
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
    let first = &live_fills[0]["content"]["data"][0];
    assert_eq!(
        trades[0].side,
        if first["isBuyerMaker"] == true {
            OrderSide::SELL
        } else {
            OrderSide::BUY
        }
    );

    // Raw channels reach `on_ws_other`.
    assert!(out.others.contains(&5) && out.lobs(5).is_empty());

    // A rejected subscription produces no events, only a warning.
    assert!(out.lobs(6).is_empty());
    assert!(
        out.logs.contains("edgeX WS error") && out.logs.contains("GATEWAY_INVALID_CONTRACT_ID"),
        "{}",
        out.logs
    );
}
