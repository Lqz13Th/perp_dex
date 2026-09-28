//! Replays frames captured from Nado through the infra runtime.

mod common;

use common::*;
use extrema_infra::prelude::*;
use perp_dex::prelude::*;
use serde_json::Value;

fn cases() -> Vec<Case> {
    let nado = || PerpDexClients::Nado(NadoCli::default());

    vec![
        Case::new(1, nado(), WsChannel::Lob(None), "@2", "nado_book"),
        Case::new(2, nado(), bbo(), "@2", "nado_bbo"),
        Case::new(3, nado(), WsChannel::Trades(None), "@2", "nado_trade"),
        Case::new(4, nado(), bbo(), "@112", "nado_stock_bbo"),
        Case::new(
            5,
            nado(),
            WsChannel::Lob(Some(LobParam::Incremental {
                depth: None,
                frequency: None,
            })),
            "@1",
            "nado_spot_book",
        ),
        Case::new(6, nado(), WsChannel::Lob(None), "@9999", "nado_bad"),
    ]
}

fn frames_of(fixture: &str, kind: &str) -> Vec<Value> {
    parsed_frames(fixture)
        .into_iter()
        .filter(|f| f["type"] == kind)
        .collect()
}

fn ns(value: &Value) -> u64 {
    value.as_str().unwrap().parse().unwrap()
}

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

fn assert_book_matches_frames(lobs: &[WsLob], frames: &[Value], inst: &str) {
    assert_eq!(lobs.len(), frames.len());
    for (lob, frame) in lobs.iter().zip(frames) {
        assert!(matches!(lob.event, LobEventKind::Incremental), "{lob:?}");
        assert!(lob.market == NADO && lob.inst == inst, "{lob:?}");
        assert_eq!(lob.timestamp, ns(&frame["max_timestamp"]) / 1_000);
        let seq = lob.seq.as_ref().unwrap();
        assert_eq!(
            (seq.prev, seq.first, seq.last),
            (
                Some(ns(&frame["last_max_timestamp"])),
                Some(ns(&frame["min_timestamp"])),
                Some(ns(&frame["max_timestamp"]))
            )
        );
        for (levels, side) in [(&lob.bids, &frame["bids"]), (&lob.asks, &frame["asks"])] {
            let side = side.as_array().unwrap();
            assert_eq!(levels.len(), side.len());
            for (level, raw) in levels.iter().zip(side) {
                assert_eq!(level.price, nado_x18_to_f64(raw[0].as_str().unwrap()));
                assert_eq!(level.size, nado_x18_to_f64(raw[1].as_str().unwrap()));
                let deleted = raw[1] == "0";
                assert_eq!(
                    matches!(level.action, LobLevelAction::Delete),
                    deleted,
                    "{level:?}"
                );
            }
        }
        assert_sides_sorted(lob);
    }
    assert_prev_chain(lobs);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn captured_nado_frames_decode_end_to_end_through_the_runtime() {
    let out = replay(cases()).await;
    eprintln!("{}", out.summary());

    assert!(
        out.unmatched.is_empty(),
        "subscribe messages differ from live: {:?}",
        out.unmatched
    );
    assert!(!out.logs.contains("Failed to deserialize"), "{}", out.logs);
    assert!(out.others.is_empty(), "no task subscribed to a raw channel");
    assert!(out.connects.values().all(|n| *n == 1), "{:?}", out.connects);

    // book_depth: no snapshot, every frame a diff chained by last_max_timestamp.
    let book_frames = frames_of("nado_book", "book_depth");
    let book = out.lobs(1);
    assert_book_matches_frames(book, &book_frames, "@2");
    assert!(
        book.iter()
            .flat_map(|l| l.bids.iter().chain(&l.asks))
            .any(|level| matches!(level.action, LobLevelAction::Delete)),
        "the fixture removes a level"
    );

    let spot_book = out.lobs(5);
    assert_book_matches_frames(spot_book, &frames_of("nado_spot_book", "book_depth"), "@1");

    // best_bid_offer: one level per side, exchange-timed.
    for (id, fixture, inst) in [(2, "nado_bbo", "@2"), (4, "nado_stock_bbo", "@112")] {
        let frames = frames_of(fixture, "best_bid_offer");
        let lobs = out.lobs(id);
        assert_eq!(lobs.len(), frames.len(), "{fixture}");
        for (lob, frame) in lobs.iter().zip(&frames) {
            assert_bbo(lob);
            assert!(lob.market == NADO && lob.inst == inst, "{lob:?}");
            assert_eq!(lob.timestamp, ns(&frame["timestamp"]) / 1_000);
            assert_eq!(
                (lob.bids[0].price, lob.bids[0].size),
                (
                    nado_x18_to_f64(frame["bid_price"].as_str().unwrap()),
                    nado_x18_to_f64(frame["bid_qty"].as_str().unwrap())
                )
            );
            assert_eq!(
                lob.asks[0].price,
                nado_x18_to_f64(frame["ask_price"].as_str().unwrap())
            );
        }
        assert!(lobs.windows(2).all(|w| w[0].timestamp <= w[1].timestamp));
    }
    let stock_mid = mid(out.lobs(4).last().unwrap());
    assert!(
        (100.0..1_000.0).contains(&stock_mid),
        "NVDA mid {stock_mid}"
    );

    // trade: one match per frame, aggressor side, no venue trade id.
    let trade_frames = frames_of("nado_trade", "trade");
    let trades = out.trades(3);
    assert_eq!(trades.len(), trade_frames.len());
    for (trade, frame) in trades.iter().zip(&trade_frames) {
        assert!(trade.market == NADO && trade.inst == "@2", "{trade:?}");
        assert_eq!(trade.timestamp, ns(&frame["timestamp"]) / 1_000);
        assert_eq!(
            trade.price,
            nado_x18_to_f64(frame["price"].as_str().unwrap())
        );
        assert_eq!(
            trade.size,
            nado_x18_to_f64(frame["taker_qty"].as_str().unwrap())
        );
        let side = if frame["is_taker_buyer"] == true {
            OrderSide::BUY
        } else {
            OrderSide::SELL
        };
        assert_eq!(trade.side, side);
        assert_eq!(trade.trade_id, 0);
        assert!(trade.price > 0.0 && trade.size > 0.0);
    }

    // Every accepted subscription is acknowledged; the rejected one only warns.
    assert_eq!(out.logs.matches("Nado WS subscribed").count(), 5);
    assert!(out.lobs(6).is_empty());
    assert!(out.logs.contains("Nado WS error"), "{}", out.logs);
    assert!(out.logs.contains("'product_id' is invalid"), "{}", out.logs);
}
