use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::json;

use extrema_infra::{
    arch::market_assets::api_general::OrderParams,
    prelude::{
        InfraError, InfraResult, LobFrequency, LobParam, OrderSide, OrderType, TimeInForce,
        TradesParam,
    },
};

/// Lighter frames carry only the market index, so every Lighter instrument is `@<market_id>`.
pub fn lighter_market_to_cli(market_id: u16) -> String {
    format!("@{market_id}")
}

pub fn cli_to_lighter_market_id(inst: &str) -> InfraResult<u16> {
    inst.strip_prefix('@')
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| {
            InfraError::ApiCliError(format!("Lighter instruments are @<market_id>, got {inst}"))
        })
}

/// `order_book:110` -> `@110`.
pub fn lighter_channel_to_cli(channel: &str) -> String {
    let market_id = channel.rsplit_once(':').map_or(channel, |(_, id)| id);
    format!("@{market_id}")
}

pub fn ws_subscribe_msg_lighter(channel: &str, market_id: Option<u16>) -> String {
    let channel = match market_id {
        Some(market_id) => format!("{channel}/{market_id}"),
        None => channel.to_string(),
    };

    json!({
        "type": "subscribe",
        "channel": channel,
    })
    .to_string()
}

pub fn lighter_lob_channel(lob_param: &Option<LobParam>) -> InfraResult<&'static str> {
    match lob_param {
        None
        | Some(LobParam::Incremental {
            depth: None,
            frequency: None,
        }) => Ok("order_book"),
        Some(LobParam::Bbo {
            frequency: None | Some(LobFrequency::Realtime),
        }) => Ok("ticker"),
        Some(param) => Err(InfraError::ApiCliError(format!(
            "Lighter pushes a full book then 50ms incremental batches, or a realtime ticker; unsupported {:?}",
            param
        ))),
    }
}

pub fn lighter_trades_channel(trades_param: &Option<TradesParam>) -> InfraResult<&'static str> {
    match trades_param {
        None | Some(TradesParam::AllTrades) => Ok("trade"),
        Some(TradesParam::AggTrades) => Err(InfraError::ApiCliError(
            "Lighter publishes individual trades only".into(),
        )),
    }
}

/// Decimal places of a market's integer base amount and price.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LighterMarketScale {
    pub size_decimals: u32,
    pub price_decimals: u32,
}

/// Exact decimal string to integer units with `decimals` places; more precision than that is an error.
pub fn lighter_scaled(value: &str, decimals: u32) -> InfraResult<i64> {
    let err = || {
        InfraError::ApiCliError(format!(
            "{value} is not a non-negative decimal with <= {decimals} places"
        ))
    };
    let v = value.trim();
    let (int, frac) = v.split_once('.').unwrap_or((v, ""));
    if (int.is_empty() && frac.is_empty())
        || !int.bytes().all(|b| b.is_ascii_digit())
        || !frac.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(err());
    }
    let d = decimals as usize;
    if frac.len() > d && frac[d..].bytes().any(|b| b != b'0') {
        return Err(err());
    }
    let mut digits = String::with_capacity(int.len() + d);
    digits.push_str(int);
    digits.push_str(&frac[..frac.len().min(d)]);
    digits.extend(std::iter::repeat_n('0', d.saturating_sub(frac.len())));
    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return Ok(0);
    }
    digits.parse().map_err(|_| err())
}

/// Market and index (exchange order index, else client order index) of an order to cancel.
pub fn lighter_cancel_target(
    inst: &str,
    order_id: Option<&str>,
    cli_order_id: Option<&str>,
) -> InfraResult<(i16, i64)> {
    let market_index = cli_to_lighter_market_id(inst)? as i16;
    let id = order_id
        .filter(|s| !s.is_empty())
        .or(cli_order_id)
        .ok_or_else(|| InfraError::ApiCliError("Lighter cancel needs an order id".into()))?;
    let index = id.parse().map_err(|_| {
        InfraError::ApiCliError(format!("Lighter order ids are integers, got {id}"))
    })?;
    Ok((market_index, index))
}

/// `CreateOrder` of one order: size and price scaled exactly to the market, every order priced (worst price for
/// `Market`), resting orders (`Limit`, `PostOnly`) expiring `order_ttl_ms` after `now_ms`, client ids below 2^48.
pub fn lighter_order_from_params(
    p: &OrderParams,
    scale: LighterMarketScale,
    header: TxHeader,
    now_ms: i64,
    order_ttl_ms: i64,
) -> InfraResult<LighterCreateOrderTx> {
    p.validate_side_and_type()?;
    let market_index = cli_to_lighter_market_id(&p.inst)? as i16;
    let price = p.price.as_deref().ok_or_else(|| {
        InfraError::ApiCliError(
            "Lighter orders need a price (worst price for market orders)".into(),
        )
    })?;
    let price = u32::try_from(lighter_scaled(price, scale.price_decimals)?)
        .map_err(|_| InfraError::ApiCliError(format!("Lighter price {price} out of range")))?;
    let (order_type, time_in_force, order_expiry) = match (&p.order_type, &p.time_in_force) {
        (OrderType::Market, _) => (
            LIGHTER_ORDER_MARKET,
            LIGHTER_TIF_IOC,
            LIGHTER_NIL_ORDER_EXPIRY,
        ),
        (OrderType::Ioc, _) | (OrderType::Limit, Some(TimeInForce::IOC)) => (
            LIGHTER_ORDER_LIMIT,
            LIGHTER_TIF_IOC,
            LIGHTER_NIL_ORDER_EXPIRY,
        ),
        (OrderType::PostOnly, _) => (
            LIGHTER_ORDER_LIMIT,
            LIGHTER_TIF_POST_ONLY,
            now_ms + order_ttl_ms,
        ),
        (OrderType::Limit, _) => (LIGHTER_ORDER_LIMIT, LIGHTER_TIF_GTT, now_ms + order_ttl_ms),
        (other, _) => {
            return Err(InfraError::ApiCliError(format!(
                "Lighter does not support {other:?} orders"
            )));
        },
    };
    let client_order_index = match p.client_order_id.as_deref() {
        Some(c) => c
            .parse::<i64>()
            .ok()
            .filter(|c| (0..=LIGHTER_MAX_CLIENT_ORDER_INDEX).contains(c))
            .ok_or_else(|| {
                InfraError::ApiCliError(format!(
                    "Lighter client order ids are integers 0..2^48, got {c}"
                ))
            })?,
        None => 0,
    };
    Ok(LighterCreateOrderTx {
        account_index: header.account_index,
        api_key_index: header.api_key_index,
        market_index,
        client_order_index,
        base_amount: lighter_scaled(&p.size, scale.size_decimals)?,
        price,
        is_ask: u8::from(p.side == OrderSide::SELL),
        order_type,
        time_in_force,
        reduce_only: u8::from(p.reduce_only.unwrap_or(false)),
        trigger_price: 0,
        order_expiry,
        expired_at: header.expired_at,
        nonce: header.nonce,
        ..Default::default()
    })
}

// Transactions sent through `sendTx` / `sendTxBatch`: the signed `tx_info` JSON (field names and order as
// lighter-go's `json.Marshal`) and the field list each one is hashed over (signing is in `auth`).

pub const LIGHTER_TX_CREATE_ORDER: u8 = 14;
pub const LIGHTER_TX_CANCEL_ORDER: u8 = 15;
pub const LIGHTER_TX_CANCEL_ALL_ORDERS: u8 = 16;
pub const LIGHTER_TX_UPDATE_LEVERAGE: u8 = 20;
pub const LIGHTER_TX_UPDATE_MARGIN: u8 = 29;

pub const LIGHTER_ORDER_LIMIT: u8 = 0;
pub const LIGHTER_ORDER_MARKET: u8 = 1;
pub const LIGHTER_TIF_IOC: u8 = 0;
pub const LIGHTER_TIF_GTT: u8 = 1;
pub const LIGHTER_TIF_POST_ONLY: u8 = 2;
pub const LIGHTER_NIL_ORDER_EXPIRY: i64 = 0;
pub const LIGHTER_CROSS_MARGIN: u8 = 0;
pub const LIGHTER_ISOLATED_MARGIN: u8 = 1;
pub const LIGHTER_MARGIN_REMOVE: u8 = 0;
pub const LIGHTER_MARGIN_ADD: u8 = 1;
pub const LIGHTER_CANCEL_ALL_IMMEDIATE: u8 = 0;
pub const LIGHTER_MAX_CLIENT_ORDER_INDEX: i64 = (1 << 48) - 1;

/// One transaction ready for `sendTx`.
#[derive(Clone, Debug)]
pub struct SignedLighterTx {
    pub tx_type: u8,
    pub tx_info: String,
    pub tx_hash: String,
}

#[derive(Clone, Copy, Debug)]
pub struct TxHeader {
    pub account_index: i64,
    pub api_key_index: u8,
    pub expired_at: i64,
    pub nonce: i64,
}

pub trait LighterTx: Serialize {
    const TX_TYPE: u8;

    fn header(&self) -> TxHeader;
    /// Fields after `chain id, tx type, nonce, expired at, account index, api key index`.
    fn body_fields(&self) -> Vec<u64>;
    fn set_sig(&mut self, sig: Vec<u8>);

    fn hash_fields(&self, chain_id: u32) -> Vec<u64> {
        let h = self.header();
        let mut fields = vec![
            u64::from(chain_id),
            u64::from(Self::TX_TYPE),
            h.nonce as u64,
            h.expired_at as u64,
            h.account_index as u64,
            u64::from(h.api_key_index),
        ];
        fields.extend(self.body_fields());
        fields
    }
}

fn ser_sig<S: Serializer>(sig: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&BASE64.encode(sig))
}

fn de_sig<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
    match Option::<String>::deserialize(d)? {
        Some(s) => BASE64.decode(s).map_err(serde::de::Error::custom),
        None => Ok(Vec::new()),
    }
}

/// lighter-go's `L2TxAttributes` map; always empty here, marshalled as `null`.
type NoAttributes = Option<()>;

macro_rules! lighter_tx_header {
    () => {
        fn header(&self) -> TxHeader {
            TxHeader {
                account_index: self.account_index,
                api_key_index: self.api_key_index,
                expired_at: self.expired_at,
                nonce: self.nonce,
            }
        }

        fn set_sig(&mut self, sig: Vec<u8>) {
            self.sig = sig;
        }
    };
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LighterCreateOrderTx {
    pub account_index: i64,
    pub api_key_index: u8,
    pub market_index: i16,
    pub client_order_index: i64,
    pub base_amount: i64,
    pub price: u32,
    pub is_ask: u8,
    #[serde(rename = "Type")]
    pub order_type: u8,
    pub time_in_force: u8,
    pub reduce_only: u8,
    pub trigger_price: u32,
    pub order_expiry: i64,
    pub expired_at: i64,
    pub nonce: i64,
    #[serde(serialize_with = "ser_sig", deserialize_with = "de_sig")]
    pub sig: Vec<u8>,
    #[serde(rename = "L2TxAttributes")]
    pub attributes: NoAttributes,
}

impl LighterTx for LighterCreateOrderTx {
    const TX_TYPE: u8 = LIGHTER_TX_CREATE_ORDER;

    lighter_tx_header!();

    fn body_fields(&self) -> Vec<u64> {
        vec![
            self.market_index as u64,
            self.client_order_index as u64,
            self.base_amount as u64,
            u64::from(self.price),
            u64::from(self.is_ask),
            u64::from(self.order_type),
            u64::from(self.time_in_force),
            u64::from(self.reduce_only),
            u64::from(self.trigger_price),
            self.order_expiry as u64,
        ]
    }
}

/// `index` is the exchange order index or the client order index.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LighterCancelOrderTx {
    pub account_index: i64,
    pub api_key_index: u8,
    pub market_index: i16,
    pub index: i64,
    pub expired_at: i64,
    pub nonce: i64,
    #[serde(serialize_with = "ser_sig", deserialize_with = "de_sig")]
    pub sig: Vec<u8>,
    #[serde(rename = "L2TxAttributes")]
    pub attributes: NoAttributes,
}

impl LighterTx for LighterCancelOrderTx {
    const TX_TYPE: u8 = LIGHTER_TX_CANCEL_ORDER;

    lighter_tx_header!();

    fn body_fields(&self) -> Vec<u64> {
        vec![self.market_index as u64, self.index as u64]
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LighterCancelAllOrdersTx {
    pub account_index: i64,
    pub api_key_index: u8,
    pub time_in_force: u8,
    pub time: i64,
    pub expired_at: i64,
    pub nonce: i64,
    #[serde(serialize_with = "ser_sig", deserialize_with = "de_sig")]
    pub sig: Vec<u8>,
    #[serde(rename = "L2TxAttributes")]
    pub attributes: NoAttributes,
}

impl LighterTx for LighterCancelAllOrdersTx {
    const TX_TYPE: u8 = LIGHTER_TX_CANCEL_ALL_ORDERS;

    lighter_tx_header!();

    fn body_fields(&self) -> Vec<u64> {
        vec![u64::from(self.time_in_force), self.time as u64]
    }
}

/// `initial_margin_fraction` is in 1/10000: 2000 = 20% = 5x.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LighterUpdateLeverageTx {
    pub account_index: i64,
    pub api_key_index: u8,
    pub market_index: i16,
    pub initial_margin_fraction: u16,
    pub margin_mode: u8,
    pub expired_at: i64,
    pub nonce: i64,
    #[serde(serialize_with = "ser_sig", deserialize_with = "de_sig")]
    pub sig: Vec<u8>,
    #[serde(rename = "L2TxAttributes")]
    pub attributes: NoAttributes,
}

impl LighterTx for LighterUpdateLeverageTx {
    const TX_TYPE: u8 = LIGHTER_TX_UPDATE_LEVERAGE;

    lighter_tx_header!();

    fn body_fields(&self) -> Vec<u64> {
        vec![
            self.market_index as u64,
            u64::from(self.initial_margin_fraction),
            u64::from(self.margin_mode),
        ]
    }
}

/// Moves isolated margin of one market; `usdc_amount` in 1e-6 USDC.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct LighterUpdateMarginTx {
    pub account_index: i64,
    pub api_key_index: u8,
    pub market_index: i16,
    #[serde(rename = "USDCAmount")]
    pub usdc_amount: i64,
    pub direction: u8,
    pub expired_at: i64,
    pub nonce: i64,
    #[serde(serialize_with = "ser_sig", deserialize_with = "de_sig")]
    pub sig: Vec<u8>,
    #[serde(rename = "L2TxAttributes")]
    pub attributes: NoAttributes,
}

impl LighterTx for LighterUpdateMarginTx {
    const TX_TYPE: u8 = LIGHTER_TX_UPDATE_MARGIN;

    lighter_tx_header!();

    fn body_fields(&self) -> Vec<u64> {
        vec![
            self.market_index as u64,
            (self.usdc_amount & 0xFFFF_FFFF) as u64,
            (self.usdc_amount >> 32) as u64,
            u64::from(self.direction),
        ]
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn market_ids_round_trip_through_cli_names() {
        assert_eq!(lighter_market_to_cli(110), "@110");
        assert_eq!(cli_to_lighter_market_id("@110").unwrap(), 110);
        assert_eq!(cli_to_lighter_market_id("@0").unwrap(), 0);
        assert_eq!(lighter_channel_to_cli("order_book:110"), "@110");
        assert_eq!(lighter_channel_to_cli("trade:2048"), "@2048");
    }

    #[test]
    fn non_index_instruments_are_rejected() {
        for inst in ["NVDA", "NVDA_USDC_PERP", "@", "@-1", "@70000", "110"] {
            assert!(cli_to_lighter_market_id(inst).is_err(), "{inst}");
        }
    }

    #[test]
    fn subscribe_msg_appends_the_market() {
        let msg: Value =
            serde_json::from_str(&ws_subscribe_msg_lighter("order_book", Some(110))).unwrap();
        assert_eq!(msg["type"], "subscribe");
        assert_eq!(msg["channel"], "order_book/110");

        let msg: Value =
            serde_json::from_str(&ws_subscribe_msg_lighter("market_stats/all", None)).unwrap();
        assert_eq!(msg["channel"], "market_stats/all");
    }

    #[test]
    fn lob_params_map_to_channels() {
        assert_eq!(lighter_lob_channel(&None).unwrap(), "order_book");
        assert_eq!(
            lighter_lob_channel(&Some(LobParam::Incremental {
                depth: None,
                frequency: None
            }))
            .unwrap(),
            "order_book"
        );
        assert_eq!(
            lighter_lob_channel(&Some(LobParam::Bbo { frequency: None })).unwrap(),
            "ticker"
        );
        assert_eq!(
            lighter_lob_channel(&Some(LobParam::Bbo {
                frequency: Some(LobFrequency::Realtime)
            }))
            .unwrap(),
            "ticker"
        );
    }

    #[test]
    fn unsupported_lob_params_are_rejected() {
        let params = [
            LobParam::Snapshot {
                depth: Some(20),
                frequency: None,
            },
            LobParam::Incremental {
                depth: Some(20),
                frequency: None,
            },
            LobParam::Incremental {
                depth: None,
                frequency: Some(LobFrequency::Ms100),
            },
            LobParam::Bbo {
                frequency: Some(LobFrequency::Ms100),
            },
        ];

        for param in params {
            assert!(
                lighter_lob_channel(&Some(param.clone())).is_err(),
                "{param:?}"
            );
        }
    }

    #[test]
    fn trades_are_individual_only() {
        assert_eq!(lighter_trades_channel(&None).unwrap(), "trade");
        assert_eq!(
            lighter_trades_channel(&Some(TradesParam::AllTrades)).unwrap(),
            "trade"
        );
        assert!(lighter_trades_channel(&Some(TradesParam::AggTrades)).is_err());
    }

    use crate::exchange::lighter::auth::{
        LighterAuth, encode_hex, lighter_hash_fields, lighter_verify,
        tests::{arr, vectors},
    };

    #[test]
    fn decimals_scale_exactly() {
        assert_eq!(lighter_scaled("1552.96", 2).unwrap(), 155296);
        assert_eq!(lighter_scaled("0.0072", 4).unwrap(), 72);
        assert_eq!(lighter_scaled("0.00720", 4).unwrap(), 72);
        assert_eq!(lighter_scaled("12", 3).unwrap(), 12000);
        assert_eq!(lighter_scaled(".5", 1).unwrap(), 5);
        assert_eq!(lighter_scaled("0", 4).unwrap(), 0);
        assert!(lighter_scaled("0.00725", 4).is_err());
        assert!(lighter_scaled("-1", 2).is_err());
        assert!(lighter_scaled("1e3", 2).is_err());
        assert!(lighter_scaled("", 2).is_err());
    }

    #[test]
    fn cancel_targets_prefer_the_exchange_index() {
        assert_eq!(
            lighter_cancel_target("@139", Some("39687971468506323"), Some("7")).unwrap(),
            (139, 39687971468506323)
        );
        assert_eq!(
            lighter_cancel_target("@139", None, Some("7")).unwrap(),
            (139, 7)
        );
        assert_eq!(
            lighter_cancel_target("@139", Some(""), Some("7")).unwrap(),
            (139, 7)
        );
        assert!(lighter_cancel_target("@139", None, None).is_err());
        assert!(lighter_cancel_target("SNDK", Some("1"), None).is_err());
    }

    #[test]
    fn order_params_map_to_create_order_fields() {
        let scale = LighterMarketScale {
            size_decimals: 4,
            price_decimals: 2,
        };
        let header = TxHeader {
            account_index: 758666,
            api_key_index: 4,
            expired_at: 1_000_599_000,
            nonce: 9,
        };
        let p = OrderParams {
            inst: "@139".into(),
            side: OrderSide::BUY,
            size: "0.0072".into(),
            order_type: OrderType::PostOnly,
            price: Some("1552.96".into()),
            reduce_only: Some(false),
            margin_mode: None,
            position_side: None,
            time_in_force: None,
            client_order_id: Some("1791518274".into()),
            extra: Default::default(),
        };
        let tx = lighter_order_from_params(&p, scale, header, 1_000_000_000, 5_000).unwrap();
        assert_eq!(
            (tx.market_index, tx.base_amount, tx.price),
            (139, 72, 155296)
        );
        assert_eq!(
            (tx.is_ask, tx.order_type, tx.time_in_force),
            (0, LIGHTER_ORDER_LIMIT, LIGHTER_TIF_POST_ONLY)
        );
        assert_eq!(
            (tx.client_order_index, tx.nonce, tx.account_index),
            (1791518274, 9, 758666)
        );
        assert_eq!(
            (tx.order_expiry, tx.expired_at),
            (1_000_005_000, 1_000_599_000)
        );

        let ioc = OrderParams {
            order_type: OrderType::Ioc,
            side: OrderSide::SELL,
            reduce_only: Some(true),
            ..p.clone()
        };
        let tx = lighter_order_from_params(&ioc, scale, header, 1_000_000_000, 5_000).unwrap();
        assert_eq!(
            (tx.is_ask, tx.time_in_force, tx.order_expiry, tx.reduce_only),
            (1, LIGHTER_TIF_IOC, 0, 1)
        );

        let fine = OrderParams {
            price: Some("1552.965".into()),
            ..p.clone()
        };
        assert!(lighter_order_from_params(&fine, scale, header, 0, 0).is_err());
        let fok = OrderParams {
            order_type: OrderType::Fok,
            ..p.clone()
        };
        assert!(lighter_order_from_params(&fok, scale, header, 0, 0).is_err());
        let big_id = OrderParams {
            client_order_id: Some((1u64 << 48).to_string()),
            ..p
        };
        assert!(lighter_order_from_params(&big_id, scale, header, 0, 0).is_err());
    }

    /// Rebuilds every golden transaction from its tx_info, then checks the type, the hash, the exact JSON,
    /// the official signature and a fresh signature of ours.
    fn check<T: LighterTx + for<'de> Deserialize<'de>>(name: &str) {
        let v = vectors();
        let tv = v.txs.iter().find(|t| t.name == name).unwrap();
        let tx: T = serde_json::from_str(&tv.tx_info).unwrap();
        assert_eq!(T::TX_TYPE, tv.tx_type, "{name}");
        assert_eq!(
            encode_hex(&lighter_hash_fields(&tx.hash_fields(v.chain_id))),
            tv.hash,
            "{name}"
        );
        assert_eq!(serde_json::to_string(&tx).unwrap(), tv.tx_info, "{name}");

        let pk: [u8; 40] = arr(&v.public_key);
        let official: Value = serde_json::from_str(&tv.tx_info).unwrap();
        let official_sig = BASE64.decode(official["Sig"].as_str().unwrap()).unwrap();
        assert!(
            lighter_verify(
                &pk,
                &arr(&tv.hash),
                &official_sig.clone().try_into().unwrap()
            ),
            "{name}"
        );

        let auth = LighterAuth::new(758666, 4, &v.private_key).unwrap();
        let signed = auth.sign_tx(tx, v.chain_id).unwrap();
        assert_eq!(signed.tx_hash, tv.hash, "{name}");
        let ours: Value = serde_json::from_str(&signed.tx_info).unwrap();
        let ours_sig = BASE64.decode(ours["Sig"].as_str().unwrap()).unwrap();
        assert_ne!(ours_sig, official_sig, "{name}: Schnorr nonces are random");
        assert!(
            lighter_verify(&pk, &arr(&tv.hash), &ours_sig.try_into().unwrap()),
            "{name}"
        );
    }

    #[test]
    fn create_orders_match_lighter_go() {
        for name in [
            "limit_post_only_buy",
            "limit_gtt_sell",
            "ioc_buy",
            "ioc_reduce_only_sell",
        ] {
            check::<LighterCreateOrderTx>(name);
        }
    }

    #[test]
    fn cancels_match_lighter_go() {
        check::<LighterCancelOrderTx>("cancel_by_order_index");
        check::<LighterCancelOrderTx>("cancel_by_client_index");
        check::<LighterCancelAllOrdersTx>("cancel_all_immediate");
    }

    #[test]
    fn leverage_and_margin_match_lighter_go() {
        check::<LighterUpdateLeverageTx>("update_leverage_isolated_5x");
        check::<LighterUpdateMarginTx>("update_margin_add");
        check::<LighterUpdateMarginTx>("update_margin_large");
    }
}
