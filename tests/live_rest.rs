//! Public REST checks against the live venues: `cargo test --test live_rest -- --ignored`.

mod common;

use std::{collections::HashSet, sync::Arc};

use common::*;
use extrema_infra::{arch::market_assets::exchange::prelude::HyperliquidCli, prelude::*};
use perp_dex::prelude::*;
use reqwest::Client;

fn shared_client() -> Arc<Client> {
    Arc::new(Client::new())
}

#[tokio::test]
#[ignore = "hits the live Lighter API"]
async fn lighter_public_rest() {
    let cli = LighterCli::new(shared_client());

    let perps = cli
        .get_instrument_info(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert_sane_instruments(&perps);
    for info in &perps {
        let id = cli_to_lighter_market_id(&info.inst).unwrap();
        assert_eq!(lighter_market_to_cli(id), info.inst);
        assert!(info.inst_code.is_some());
    }
    let nvda = perps
        .iter()
        .find(|i| i.inst_code.as_deref() == Some("NVDA"))
        .expect("Lighter lists NVDA");
    assert_eq!(nvda.inst, "@110");
    assert!(nvda.max_leverage.is_some() && nvda.min_notional.is_some());

    let spots = cli.get_instrument_info(InstrumentType::Spot).await.unwrap();
    assert!(!spots.is_empty());
    assert!(spots.iter().all(|i| i.inst_type == InstrumentType::Spot));

    let live = cli
        .get_live_instruments(InstrumentType::Perpetual)
        .await
        .unwrap();
    let perp_insts: HashSet<_> = perps.iter().map(|i| i.inst.clone()).collect();
    assert!(
        !live.is_empty() && live.len() < perps.len(),
        "some markets are reduce-only or inactive"
    );
    assert!(live.iter().all(|i| perp_insts.contains(i)));

    let details = cli.get_order_book_details(None).await.unwrap();
    assert!(!details.order_book_details.is_empty() && !details.spot_order_book_details.is_empty());
    let one = cli.get_order_book_details(Some(110)).await.unwrap();
    let markets: Vec<_> = one.into_markets().collect();
    assert_eq!(markets.len(), 1);
    assert_eq!(
        (markets[0].symbol.as_str(), markets[0].inst()),
        ("NVDA", "@110".to_string())
    );
    assert!(cli.get_order_book_details(Some(9999)).await.is_err());

    let insts = vec!["@110".to_string(), "@1".to_string()];
    let tickers = cli.get_tickers(Some(&insts), None).await.unwrap();
    assert_eq!(
        tickers
            .iter()
            .map(|t| t.inst.clone())
            .collect::<HashSet<_>>(),
        insts.iter().cloned().collect()
    );
    assert!(tickers.iter().all(|t| t.price > 0.0));
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

    let marks = cli.get_mark_prices(Some(&insts), None).await.unwrap();
    assert_eq!(marks.len(), 2);
    for mark in &marks {
        let last = tickers.iter().find(|t| t.inst == mark.inst).unwrap().price;
        assert!(
            (mark.mark_price / last - 1.0).abs() < 0.05,
            "{mark:?} vs {last}"
        );
    }
    assert!(cli.get_mark_prices(None, None).await.unwrap().len() >= perps.len() / 2);

    let book = cli
        .get_orderbook("@110", InstrumentType::Perpetual, 5)
        .await
        .unwrap();
    assert_sane_book(&book, "@110", 5);
    let deep = cli
        .get_orderbook("@1", InstrumentType::Perpetual, 0)
        .await
        .unwrap();
    assert_sane_book(&deep, "@1", 250);
    assert!(deep.bids.len() > 5);
    assert!(
        cli.get_orderbook("NVDA", InstrumentType::Perpetual, 5)
            .await
            .is_err()
    );
}

#[tokio::test]
#[ignore = "hits the live Lighter Robinhood Chain API"]
async fn lighter_robinhood_public_rest() {
    let mut cli = LighterCli::new(shared_client());
    cli.set_venue(LighterVenue::Robinhood);

    let perps = cli
        .get_instrument_info(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert_sane_instruments(&perps);
    let spy = perps
        .iter()
        .find(|i| i.inst_code.as_deref() == Some("SPY"))
        .expect("Lighter RH lists SPY");
    let mainnet_nvda = LighterCli::new(shared_client())
        .get_order_book_details(Some(110))
        .await
        .unwrap();
    assert_eq!(mainnet_nvda.order_book_details[0].symbol, "NVDA");
    let rh_110 = cli.get_order_book_details(Some(110)).await;
    assert!(
        rh_110.is_err() || rh_110.unwrap().order_book_details[0].symbol != "NVDA",
        "market ids are per venue"
    );

    let live = cli
        .get_live_instruments(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert!(live.contains(&spy.inst));
    let insts = vec![spy.inst.clone()];
    let tickers = cli.get_tickers(Some(&insts), None).await.unwrap();
    let marks = cli.get_mark_prices(Some(&insts), None).await.unwrap();
    assert_eq!((tickers.len(), marks.len()), (1, 1));
    assert!((marks[0].mark_price / tickers[0].price - 1.0).abs() < 0.05);
    let book = cli
        .get_orderbook(&spy.inst, InstrumentType::Perpetual, 5)
        .await
        .unwrap();
    assert_sane_book(&book, &spy.inst, 5);
    assert!((book.bids[0].0 / marks[0].mark_price - 1.0).abs() < 0.05);
}

#[tokio::test]
#[ignore = "hits the live Aster API"]
async fn aster_public_rest() {
    let cli = AsterCli::new(shared_client());

    let info = cli.get_exchange_info().await.unwrap();
    for symbol in &info.symbols {
        assert_eq!(
            aster_inst_to_cli(&symbol.symbol),
            format!("{}_{}_PERP", symbol.baseAsset, symbol.quoteAsset),
            "symbol parsing disagrees with exchangeInfo for {}",
            symbol.symbol
        );
        assert_eq!(
            cli_perp_to_aster_symbol(&aster_inst_to_cli(&symbol.symbol)),
            symbol.symbol
        );
    }
    let stocks: Vec<_> = info.symbols.iter().filter(|s| s.is_stock()).collect();
    assert!(stocks.len() > 50, "{} stock perps", stocks.len());
    assert!(stocks.iter().any(|s| s.symbol == "NVDAUSDT"));

    let perps = cli
        .get_instrument_info(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert_sane_instruments(&perps);
    assert!(
        cli.get_instrument_info(InstrumentType::Spot)
            .await
            .unwrap()
            .is_empty()
    );
    let nvda = perps.iter().find(|i| i.inst == "NVDA_USDT_PERP").unwrap();
    assert_eq!(nvda.state, InstrumentStatus::Live);
    assert_eq!(nvda.min_notional, Some(5.0));

    let live = cli
        .get_live_instruments(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert!(live.contains(&"NVDA_USDT_PERP".to_string()) && live.len() < perps.len());

    let insts = vec!["NVDA_USDT_PERP".to_string(), "BTC_USDT_PERP".to_string()];
    let tickers = cli.get_tickers(Some(&insts), None).await.unwrap();
    assert_eq!(tickers.len(), 2);
    assert!(
        tickers
            .iter()
            .all(|t| t.price > 0.0 && t.timestamp > 1_700_000_000_000_000)
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

    let premium = cli.get_premium_index(Some("NVDA_USDT_PERP")).await.unwrap();
    assert_eq!(premium.len(), 1);
    assert_eq!(premium[0].symbol, "NVDAUSDT");
    assert!(cli.get_premium_index(None).await.unwrap().len() > 100);
    let funding = cli
        .get_funding_rate_live(Some("NVDA_USDT_PERP"))
        .await
        .unwrap();
    assert_eq!(funding.len(), 1);
    assert!(funding[0].funding_time > funding[0].timestamp);
    let intervals = cli.get_funding_info().await.unwrap();
    assert!(!intervals.is_empty());
    assert!(
        intervals
            .iter()
            .all(|i| [3_600.0, 7_200.0, 14_400.0, 28_800.0].contains(&i.funding_interval_sec))
    );

    let book = cli
        .get_orderbook("NVDA_USDT_PERP", InstrumentType::Perpetual, 5)
        .await
        .unwrap();
    assert_sane_book(&book, "NVDA_USDT_PERP", 5);
    let deep = cli
        .get_orderbook("BTC_USDT_PERP", InstrumentType::Perpetual, 0)
        .await
        .unwrap();
    assert_sane_book(&deep, "BTC_USDT_PERP", 500);
    assert!(
        cli.get_orderbook("NVDA_USDT_PERP", InstrumentType::Perpetual, 7)
            .await
            .is_err()
    );
    assert!(
        cli.get_orderbook("NVDA_USDT_PERP", InstrumentType::Spot, 5)
            .await
            .is_err()
    );
    let err = cli
        .get_orderbook("NOPE_USDT_PERP", InstrumentType::Perpetual, 5)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Aster REST error"), "{err}");
}

#[tokio::test]
#[ignore = "hits the live Arcus API"]
async fn arcus_public_rest() {
    let cli = ArcusCli::new(shared_client());

    let markets = cli.get_markets().await.unwrap();
    for market in &markets {
        assert_eq!(
            cli_perp_to_arcus_market(&market.inst()).unwrap(),
            market.marketDisplayName
        );
    }
    let equities: Vec<_> = markets.iter().filter(|m| m.is_equity()).collect();
    assert!(equities.len() > 10);
    let nvda = equities
        .iter()
        .find(|m| m.marketDisplayName == "NVDA-USD")
        .unwrap();
    assert!(nvda.regularTradingHours.is_some());

    let perps = cli
        .get_instrument_info(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert_sane_instruments(&perps);
    assert_eq!(perps.len(), markets.len());
    assert!(
        cli.get_instrument_info(InstrumentType::Spot)
            .await
            .unwrap()
            .is_empty()
    );
    let info = perps.iter().find(|i| i.inst == "NVDA_USD_PERP").unwrap();
    assert_eq!(info.inst_code, Some(nvda.marketId.to_string()));

    let live = cli
        .get_live_instruments(InstrumentType::Perpetual)
        .await
        .unwrap();
    assert!(live.contains(&"NVDA_USD_PERP".to_string()));
    assert_eq!(
        live.len(),
        markets.iter().filter(|m| m.status == "ONLINE").count()
    );

    let insts = vec!["NVDA_USD_PERP".to_string(), "BTC_USD_PERP".to_string()];
    let tickers = cli.get_tickers(Some(&insts), None).await.unwrap();
    assert_eq!(tickers.len(), 2);
    let marks = cli
        .get_mark_prices(Some(&insts), Some(InstrumentType::Perpetual))
        .await
        .unwrap();
    assert_eq!(marks.len(), 2);
    for mark in &marks {
        let last = tickers.iter().find(|t| t.inst == mark.inst).unwrap().price;
        assert!(
            (mark.mark_price / last - 1.0).abs() < 0.05,
            "{mark:?} vs {last}"
        );
    }
    assert!(
        cli.get_tickers(None, Some(InstrumentType::Spot))
            .await
            .unwrap()
            .is_empty()
    );

    let book = cli
        .get_orderbook("NVDA_USD_PERP", InstrumentType::Perpetual, 5)
        .await
        .unwrap();
    assert_sane_book(&book, "NVDA_USD_PERP", 5);
    let deep = cli
        .get_orderbook("BTC_USD_PERP", InstrumentType::Perpetual, 0)
        .await
        .unwrap();
    assert_sane_book(&deep, "BTC_USD_PERP", 100);
    assert!(
        cli.get_orderbook("NVDA_USD_PERP", InstrumentType::Perpetual, 101)
            .await
            .is_err()
    );
    assert!(
        cli.get_orderbook("NVDA-USD", InstrumentType::Perpetual, 5)
            .await
            .is_err()
    );
    let err = cli
        .get_orderbook("NOPE_USD_PERP", InstrumentType::Perpetual, 5)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Unknown market"), "{err}");
}

#[tokio::test]
#[ignore = "hits all live venues"]
async fn stock_perps_overlap_across_venues() {
    let client = shared_client();
    let lighter: HashSet<String> = LighterCli::new(client.clone())
        .get_instrument_info(InstrumentType::Perpetual)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|i| i.inst_code)
        .collect();
    let aster: HashSet<String> = AsterCli::new(client.clone())
        .get_exchange_info()
        .await
        .unwrap()
        .symbols
        .into_iter()
        .filter(|s| s.is_stock() && s.quoteAsset == "USDT")
        .map(|s| s.baseAsset)
        .collect();
    let arcus: HashSet<String> = ArcusCli::new(client)
        .get_markets()
        .await
        .unwrap()
        .into_iter()
        .filter(|m| m.is_equity())
        .map(|m| m.baseAsset)
        .collect();

    let all_three: Vec<_> = aster
        .iter()
        .filter(|s| lighter.contains(*s) && arcus.contains(*s))
        .collect();
    eprintln!(
        "stocks listed on Lighter, Aster and Arcus: {} {:?}",
        all_three.len(),
        all_three
    );
    assert!(all_three.iter().any(|s| s.as_str() == "NVDA"));
}

#[tokio::test]
#[ignore = "hits all live venues"]
async fn perp_dex_clients_dispatch_to_each_venue() {
    let client = shared_client();
    let mut xyz = HyperliquidCli::new(client.clone());
    xyz.set_perp_dex(Some("xyz".into()));
    xyz.init_inst_index_map().await.unwrap();

    let venues = [
        (PerpDexClients::Hyperliquid(xyz), "NVDA_USDC_PERP"),
        (
            PerpDexClients::Lighter(LighterCli::new(client.clone())),
            "@110",
        ),
        (
            PerpDexClients::Aster(AsterCli::new(client.clone())),
            "NVDA_USDT_PERP",
        ),
        (
            PerpDexClients::Arcus(ArcusCli::new(client)),
            "NVDA_USD_PERP",
        ),
    ];

    for (venue, inst) in &venues {
        let market = venue.market();
        let infos = venue
            .get_instrument_info(InstrumentType::Perpetual)
            .await
            .unwrap();
        assert!(
            infos.iter().any(|i| i.inst == *inst),
            "{market:?} lacks {inst}"
        );

        let book = venue
            .get_orderbook(inst, InstrumentType::Perpetual, 5)
            .await
            .unwrap();
        assert_sane_book(&book, inst, 5);

        let marks = venue
            .get_mark_prices(Some(&[inst.to_string()]), Some(InstrumentType::Perpetual))
            .await
            .unwrap();
        assert_eq!(marks.len(), 1, "{market:?}");
        let mid = (book.bids[0].0 + book.asks[0].0) / 2.0;
        assert!(
            (marks[0].mark_price / mid - 1.0).abs() < 0.05,
            "{market:?} mark far from mid"
        );

        let channel = WsChannel::Lob(Some(LobParam::Bbo { frequency: None }));
        assert!(
            venue
                .get_public_connect_msg(&channel)
                .await
                .unwrap()
                .starts_with("wss://")
        );
        assert!(
            !venue
                .get_public_sub_msg(&channel, Some(&[inst.to_string()]))
                .await
                .unwrap()
                .is_empty()
        );
    }
}
