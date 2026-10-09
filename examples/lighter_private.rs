//! Lighter private API round trip on a live account (`LIGHTER_ACCOUNT_INDEX`, `LIGHTER_API_KEY_INDEX`,
//! `LIGHTER_API_PRIVATE_KEY`): checks the key against the exchange, reads balance and positions, then posts
//! post-only bids 5% below the touch (they cannot fill) and cancels them one by one and as a batch.
//!
//! `cargo run --example lighter_private --features lighter -- @139`            read-only
//! `cargo run --example lighter_private --features lighter -- @139 orders`     plus the order round trip

use std::time::Duration;

use extrema_infra::{
    arch::market_assets::api_general::{CancelOrderParams, OrderParams, get_micros_timestamp},
    prelude::*,
};
use perp_dex::prelude::*;

fn post_only_bid(inst: &str, size: String, price: String, cli_order_id: u64) -> OrderParams {
    OrderParams {
        inst: inst.to_string(),
        side: OrderSide::BUY,
        size,
        order_type: OrderType::PostOnly,
        price: Some(price),
        reduce_only: Some(false),
        margin_mode: None,
        position_side: None,
        time_in_force: None,
        client_order_id: Some(cli_order_id.to_string()),
        extra: Default::default(),
    }
}

async fn show_open(cli: &LighterCli, inst: &str) -> InfraResult<Vec<String>> {
    tokio::time::sleep(Duration::from_secs(3)).await;
    let open = cli.get_open_orders(inst, None).await?;
    for o in &open {
        println!(
            "   open: order_id={} cli={:?} {:?} {:?} {} @ {} status={:?}",
            o.order_id, o.cli_order_id, o.side, o.order_type, o.size, o.price, o.order_status
        );
    }
    if open.is_empty() {
        println!("   open: none");
    }
    Ok(open.into_iter().map(|o| o.order_id).collect())
}

#[tokio::main]
async fn main() -> InfraResult<()> {
    let mut args = std::env::args().skip(1);
    let inst = args.next().unwrap_or_else(|| "@139".into());
    let with_orders = args.next().as_deref() == Some("orders");

    let mut cli = LighterCli::default();
    cli.init_api_key();
    cli.init_market_scales().await?;
    println!(
        "api key registered on the exchange matches the local key: {}",
        cli.check_api_key().await?
    );
    println!("next nonce: {}", cli.next_nonce().await?);
    for b in cli.get_balance(None).await? {
        println!(
            "balance {}: total {} available {}",
            b.asset, b.total, b.available
        );
    }
    for p in cli.get_positions(None).await? {
        println!(
            "position {} {} @ {} (margin {}, {}x)",
            p.inst, p.size, p.avg_price, p.margin, p.leverage
        );
    }
    show_open(&cli, &inst).await?;
    if !with_orders {
        return Ok(());
    }

    let market_id = cli_to_lighter_market_id(&inst)?;
    let scale = cli.market_scale(market_id)?;
    let bid = cli
        .get_orderbook(&inst, InstrumentType::Perpetual, 1)
        .await?
        .bids
        .first()
        .map(|l| l.0)
        .ok_or_else(|| InfraError::Msg(format!("no bid on {inst}")))?;
    let px = bid * 0.95;
    let lot = 10f64.powi(-(scale.size_decimals as i32));
    let size = ((11.0 / px) / lot).ceil() * lot;
    let fmt = |v: f64, d: u32| format!("{v:.prec$}", prec = d as usize);
    let (size, price) = (
        fmt(size, scale.size_decimals),
        fmt(px, scale.price_decimals),
    );
    let base = get_micros_timestamp() / 1_000 % 1_000_000_000;

    println!("\n1. single post-only bid {size} @ {price} (bid {bid})");
    let a = cli
        .place_order(post_only_bid(&inst, size.clone(), price.clone(), base))
        .await?;
    println!(
        "   ack: {:?} cli={:?} tx={:?}",
        a.order_status, a.cli_order_id, a.msg
    );
    let ids = show_open(&cli, &inst).await?;
    if let Some(id) = ids.first() {
        let c = cli.cancel_order(&inst, Some(id), None).await?;
        println!("   cancel by order id: {:?} tx={:?}", c.order_status, c.msg);
    }
    show_open(&cli, &inst).await?;

    println!("\n2. batch of two bids, cancelled as a batch by client id");
    let acks = cli
        .place_orders(vec![
            post_only_bid(&inst, size.clone(), price.clone(), base + 1),
            post_only_bid(&inst, size, price, base + 2),
        ])
        .await?;
    println!(
        "   acks: {:?}",
        acks.iter()
            .map(|a| (&a.cli_order_id, &a.msg))
            .collect::<Vec<_>>()
    );
    show_open(&cli, &inst).await?;
    let cancels = cli
        .cancel_orders(
            [base + 1, base + 2]
                .map(|c| CancelOrderParams {
                    inst: inst.clone(),
                    order_id: None,
                    cli_order_id: Some(c.to_string()),
                })
                .to_vec(),
        )
        .await?;
    println!(
        "   cancel acks: {:?}",
        cancels.iter().map(|a| &a.msg).collect::<Vec<_>>()
    );
    let left = show_open(&cli, &inst).await?;

    if !left.is_empty() {
        println!("   cancel all: tx {}", cli.cancel_all_orders().await?);
        show_open(&cli, &inst).await?;
    }
    println!("\nnext nonce now: {}", cli.next_nonce().await?);
    Ok(())
}
