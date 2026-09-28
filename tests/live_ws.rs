//! Live streams through the real infra runtime: `cargo test --test live_ws -- --ignored`.

mod common;

use std::time::Duration;

use common::*;
use extrema_infra::{arch::market_assets::exchange::prelude::HyperliquidCli, prelude::*};
use perp_dex::prelude::*;

const RUN_FOR: Duration = Duration::from_secs(30);

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "streams from all live venues for 30 seconds"]
async fn live_streams_decode_cleanly() {
    let mut xyz = HyperliquidCli::default();
    xyz.set_perp_dex(Some("xyz".into()));
    xyz.init_inst_index_map().await.unwrap();

    let lighter = PerpDexClients::Lighter(LighterCli::default());
    let aster = PerpDexClients::Aster(AsterCli::default());
    let arcus = PerpDexClients::Arcus(ArcusCli::default());
    let mut rh = LighterCli::default();
    rh.set_venue(LighterVenue::Robinhood);
    let lighter_rh = PerpDexClients::Lighter(rh);
    let cases = vec![
        Case::new(1, lighter.clone(), WsChannel::Lob(None), "@110", ""),
        Case::new(2, lighter.clone(), bbo(), "@110", ""),
        Case::new(3, lighter, WsChannel::Trades(None), "@1", ""),
        Case::new(4, aster.clone(), bbo(), "NVDA_USDT_PERP", ""),
        Case::new(
            5,
            aster.clone(),
            WsChannel::Lob(Some(LobParam::Incremental {
                depth: None,
                frequency: Some(LobFrequency::Ms100),
            })),
            "NVDA_USDT_PERP",
            "",
        ),
        Case::new(
            6,
            aster.clone(),
            WsChannel::Lob(Some(LobParam::Snapshot {
                depth: Some(5),
                frequency: Some(LobFrequency::Ms100),
            })),
            "NVDA_USDT_PERP",
            "",
        ),
        Case::new(
            7,
            aster,
            WsChannel::Trades(Some(TradesParam::AllTrades)),
            "BTC_USDT_PERP",
            "",
        ),
        Case::new(8, arcus.clone(), WsChannel::Lob(None), "NVDA_USD_PERP", ""),
        Case::new(
            9,
            arcus.clone(),
            WsChannel::Lob(Some(LobParam::Snapshot {
                depth: Some(5),
                frequency: None,
            })),
            "NVDA_USD_PERP",
            "",
        ),
        Case::new(10, arcus.clone(), bbo(), "NVDA_USD_PERP", ""),
        Case::new(11, arcus, WsChannel::Trades(None), "BTC_USD_PERP", ""),
        Case::new(
            12,
            PerpDexClients::Hyperliquid(xyz),
            bbo(),
            "NVDA_USDC_PERP",
            "",
        ),
        Case::new(13, lighter_rh.clone(), WsChannel::Lob(None), "@15", ""),
        Case::new(14, lighter_rh, bbo(), "@15", ""),
    ];

    let run = live(cases, RUN_FOR).await;
    eprintln!("{}", run.summary());

    assert_clean(&run);
    assert!(
        run.connects.values().all(|n| *n == 1),
        "reconnected: {:?}",
        run.connects
    );
    for id in [1, 2, 4, 5, 6, 8, 9, 10, 12, 13, 14] {
        assert!(!run.lobs(id).is_empty(), "no book events on task {id}");
    }

    let lighter_book = run.lobs(1);
    assert!(matches!(lighter_book[0].event, LobEventKind::Snapshot));
    assert_prev_chain(lighter_book);
    assert_prev_chain(run.lobs(5));
    assert!(matches!(run.lobs(13)[0].event, LobEventKind::Snapshot));
    assert_prev_chain(run.lobs(13));
    assert!(
        run.lobs(6)
            .iter()
            .all(|l| matches!(l.event, LobEventKind::Snapshot) && l.bids.len() <= 5)
    );

    let arcus_book = run.lobs(8);
    assert!(matches!(arcus_book[0].event, LobEventKind::Snapshot));
    if arcus_book.len() > 2 {
        assert_last_contiguous(&arcus_book[1..]);
    }
    assert!(
        run.lobs(9)
            .iter()
            .all(|l| matches!(l.event, LobEventKind::Snapshot) && l.timestamp > 0)
    );

    let nvda = [
        (2, LIGHTER, "@110"),
        (4, ASTER, "NVDA_USDT_PERP"),
        (10, ARCUS, "NVDA_USD_PERP"),
        (12, Market::HyperLiquid, "NVDA_USDC_PERP"),
        (14, LIGHTER_RH, "@15"),
    ];
    for (id, market, inst) in &nvda {
        run.lobs(*id).iter().for_each(assert_bbo);
        assert!(
            run.lobs(*id).iter().all(|l| l.market == *market
                && l.inst == *inst
                && l.timestamp > 1_700_000_000_000_000),
            "task {id}"
        );
    }
    let mids: Vec<f64> = nvda
        .iter()
        .map(|(id, _, _)| mid(run.lobs(*id).last().unwrap()))
        .collect();
    let lo = mids.iter().cloned().fold(f64::MAX, f64::min);
    let hi = mids.iter().cloned().fold(f64::MIN, f64::max);
    eprintln!("NVDA mids (Lighter, Aster, Arcus, HL xyz, Lighter RH): {mids:?}");
    assert!(hi / lo - 1.0 < 0.02, "NVDA mids disagree: {mids:?}");

    for trade in run.trades.values().flatten() {
        assert!(
            matches!(trade.side, OrderSide::BUY | OrderSide::SELL)
                && trade.price > 0.0
                && trade.size > 0.0
        );
    }
}
