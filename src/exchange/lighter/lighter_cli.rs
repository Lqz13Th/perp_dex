use reqwest::Client;
use serde::de::DeserializeOwned;
use std::{collections::HashMap, slice, sync::Arc};
use tracing::error;

use extrema_infra::{
    arch::market_assets::{
        api_data::{
            account_data::{BalanceData, OrderAckData, OrderDetailData, PositionData},
            price_data::*,
            utils_data::*,
        },
        api_general::{
            CancelOrderParams, OrderParams, get_micros_timestamp, get_mills_timestamp,
            parse_json_response,
        },
    },
    prelude::*,
};

use crate::exchange::api_general::{exactly_one, single_ws_inst};

use super::{
    api_utils::*,
    auth::{LighterAuth, LighterNonce, encode_hex, read_lighter_env_auth},
    config_assets::*,
    lighter_rest_msg::RestResLighter,
    schemas::rest::{
        account::{AccountLighter, RestAccountsLighter},
        account_limits::RestAccountLimitsLighter,
        api_keys::{ApiKeyLighter, RestApiKeysLighter},
        next_nonce::RestNextNonceLighter,
        open_order::RestOpenOrdersLighter,
        order_book_details::RestOrderBookDetailsLighter,
        order_book_orders::RestOrderBookOrdersLighter,
        position_funding::{PositionFundingLighter, RestPositionFundingLighter},
        trade_order::{RestSendTxBatchLighter, RestSendTxLighter},
        trades::{LighterFill, RestTradesLighter},
    },
};

/// Public market data, plus the private API once credentials (`init_api_key` / `set_auth`) and market scales
/// (`init_market_scales`) are set. Clones share the nonce counter, so they can sign for the same key concurrently.
///
/// Private notes: `OrderParams.size` / `price` are decimal strings scaled exactly to the market's integer units;
/// every order needs a price (worst price for `Market`). `sendTx` only says the sequencer accepted a transaction,
/// so acks are `Live` (`Canceled` for cancels) with the transaction hash in `msg`; fills and the exchange
/// `order_index` come from the `AccountOrders` stream or `get_open_orders`.
///
/// Websocket order entry: `sign_orders` / `sign_cancels` / `sign_modify` and a [`LIGHTER_TX_CHANNEL`] task
/// sending [`lighter_ws_send_tx_msg`] frames; after a rejected or lost transaction call `invalidate_nonce`.
#[derive(Clone, Debug)]
pub struct LighterCli {
    pub client: Arc<Client>,
    pub venue: LighterVenue,
    pub auth: Option<LighterAuth>,
    pub market_scales: HashMap<u16, LighterMarketScale>,
    pub(crate) nonce: Arc<LighterNonce>,
}

impl Default for LighterCli {
    fn default() -> Self {
        Self::new(Arc::new(Client::new()))
    }
}

impl LobPublicRest for LighterCli {
    async fn get_tickers(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<TickerData>> {
        self._get_tickers(insts, inst_type).await
    }

    async fn get_mark_prices(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<MarkPriceData>> {
        self._get_mark_prices(insts, inst_type).await
    }

    async fn get_orderbook(
        &self,
        inst: &str,
        inst_type: InstrumentType,
        depth: usize,
    ) -> InfraResult<OrderBookData> {
        self._get_orderbook(inst, inst_type, depth).await
    }

    async fn get_instrument_info(
        &self,
        inst_type: InstrumentType,
    ) -> InfraResult<Vec<InstrumentInfo>> {
        self._get_instrument_info(inst_type).await
    }

    async fn get_live_instruments(&self, inst_type: InstrumentType) -> InfraResult<Vec<String>> {
        self._get_live_instruments(inst_type).await
    }
}

impl LobPrivateRest for LighterCli {
    fn init_api_key(&mut self) {
        match read_lighter_env_auth() {
            Ok(auth) => self.set_auth(auth),
            Err(e) => error!("Failed to read LIGHTER env auth: {:?}", e),
        }
    }

    async fn place_order(&self, order_params: OrderParams) -> InfraResult<OrderAckData> {
        let tx = self.sign_orders(slice::from_ref(&order_params)).await?;
        let sent = self.invalidate_on_err(self.send_tx(&tx[0]).await)?;
        Ok(ack(
            OrderStatus::Live,
            order_params.client_order_id,
            sent.tx_hash,
        ))
    }

    async fn place_orders(&self, order_params: Vec<OrderParams>) -> InfraResult<Vec<OrderAckData>> {
        let mut acks = Vec::with_capacity(order_params.len());
        for chunk in order_params.chunks(LIGHTER_SEND_TX_BATCH_MAX) {
            let txs = self.sign_orders(chunk).await?;
            let sent = self.invalidate_on_err(self.send_tx_batch(&txs).await)?;
            for (p, hash) in chunk.iter().zip(padded(sent.tx_hash, chunk.len())) {
                acks.push(ack(OrderStatus::Live, p.client_order_id.clone(), hash));
            }
        }
        Ok(acks)
    }

    async fn cancel_order(
        &self,
        inst: &str,
        order_id: Option<&str>,
        cli_order_id: Option<&str>,
    ) -> InfraResult<OrderAckData> {
        let params = CancelOrderParams {
            inst: inst.to_string(),
            order_id: order_id.map(str::to_string),
            cli_order_id: cli_order_id.map(str::to_string),
        };
        let tx = self.sign_cancels(slice::from_ref(&params)).await?;
        let hash = self.invalidate_on_err(self.send_tx(&tx[0]).await)?.tx_hash;
        let mut a = ack(OrderStatus::Canceled, params.cli_order_id, hash);
        a.order_id = params.order_id.unwrap_or_default();
        Ok(a)
    }

    async fn cancel_orders(
        &self,
        cancel_params: Vec<CancelOrderParams>,
    ) -> InfraResult<Vec<OrderAckData>> {
        let mut acks = Vec::with_capacity(cancel_params.len());
        for chunk in cancel_params.chunks(LIGHTER_SEND_TX_BATCH_MAX) {
            let txs = self.sign_cancels(chunk).await?;
            let sent = self.invalidate_on_err(self.send_tx_batch(&txs).await)?;
            for (c, hash) in chunk.iter().zip(padded(sent.tx_hash, chunk.len())) {
                let mut a = ack(OrderStatus::Canceled, c.cli_order_id.clone(), hash);
                a.order_id = c.order_id.clone().unwrap_or_default();
                acks.push(a);
            }
        }
        Ok(acks)
    }

    async fn get_open_orders(
        &self,
        inst: &str,
        limit: Option<u32>,
    ) -> InfraResult<Vec<OrderDetailData>> {
        let query = [
            ("account_index", self.auth_ref()?.account_index.to_string()),
            ("market_id", cli_to_lighter_market_id(inst)?.to_string()),
        ];
        let res: RestOpenOrdersLighter = self
            .get_private(
                LIGHTER_ACCOUNT_ACTIVE_ORDERS,
                &query,
                "Lighter accountActiveOrders",
            )
            .await?;
        let mut orders: Vec<OrderDetailData> = res
            .orders
            .into_iter()
            .map(|o| o.into_order_detail_data())
            .collect();
        if let Some(limit) = limit {
            orders.truncate(limit as usize);
        }
        Ok(orders)
    }

    async fn get_balance(&self, assets: Option<&[String]>) -> InfraResult<Vec<BalanceData>> {
        let balance = self
            .get_account()
            .await?
            .into_balance_data(get_micros_timestamp());
        Ok(assets
            .is_none_or(|a| a.iter().any(|x| x.eq_ignore_ascii_case(&balance.asset)))
            .then_some(balance)
            .into_iter()
            .collect())
    }

    async fn get_positions(&self, insts: Option<&[String]>) -> InfraResult<Vec<PositionData>> {
        let timestamp = get_micros_timestamp();
        Ok(self
            .get_account()
            .await?
            .positions
            .iter()
            .filter(|p| p.size() != 0.0)
            .map(|p| p.into_position_data(timestamp))
            .filter(|p| insts.is_none_or(|list| list.contains(&p.inst)))
            .collect())
    }

    /// Filled, canceled and expired orders of one market, newest first; `limit: None` pages through all of
    /// them in the window.
    async fn get_order_history(
        &self,
        inst: &str,
        start_time_us: Option<u64>,
        end_time_us: Option<u64>,
        limit: Option<u32>,
    ) -> InfraResult<Vec<OrderDetailData>> {
        let mut query = vec![
            ("account_index", self.auth_ref()?.account_index.to_string()),
            ("market_id", cli_to_lighter_market_id(inst)?.to_string()),
        ];
        if start_time_us.is_some() || end_time_us.is_some() {
            let start_s = start_time_us.unwrap_or_default() / 1_000_000;
            let end_s = end_time_us
                .unwrap_or_else(get_micros_timestamp)
                .div_ceil(1_000_000);
            query.push(("between_timestamps", format!("{start_s}-{end_s}")));
        }
        let mut orders = Vec::new();
        if limit == Some(0) {
            return Ok(orders);
        }
        loop {
            let want = limit.map_or(LIGHTER_HISTORY_PAGE_MAX, |l| {
                (l - orders.len() as u32).min(LIGHTER_HISTORY_PAGE_MAX)
            });
            let mut page_query = query.clone();
            page_query.push(("limit", want.to_string()));
            let page: RestOpenOrdersLighter = self
                .get_private(
                    LIGHTER_ACCOUNT_INACTIVE_ORDERS,
                    &page_query,
                    "Lighter accountInactiveOrders",
                )
                .await?;
            let n = page.orders.len();
            orders.extend(page.orders.into_iter().map(|o| o.into_order_detail_data()));
            match page.next_cursor {
                Some(cursor)
                    if n as u32 == want && limit.is_none_or(|l| (orders.len() as u32) < l) =>
                {
                    query.retain(|(k, _)| *k != "cursor");
                    query.push(("cursor", cursor));
                },
                _ => break,
            }
        }
        Ok(orders)
    }
}

impl LobWebsocket for LighterCli {
    async fn get_public_sub_msg(
        &self,
        channel: &WsChannel,
        insts: Option<&[String]>,
    ) -> InfraResult<String> {
        self._get_public_sub_msg(channel, insts)
    }

    /// Signs a fresh auth token; the exchange checks it only when subscribing.
    async fn get_private_sub_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        self._get_private_sub_msg(channel)
    }

    async fn get_public_connect_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        self._get_public_connect_msg(channel)
    }

    async fn get_private_connect_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        self._get_private_connect_msg(channel)
    }
}

impl LighterCli {
    pub fn new(shared_client: Arc<Client>) -> Self {
        Self {
            client: shared_client,
            venue: LighterVenue::Mainnet,
            auth: None,
            market_scales: HashMap::new(),
            nonce: Arc::default(),
        }
    }

    /// Market ids differ between venues, so one client serves one venue.
    pub fn set_venue(&mut self, venue: LighterVenue) {
        self.venue = venue;
        self.nonce = Arc::default();
        self.market_scales.clear();
    }

    pub fn set_auth(&mut self, auth: LighterAuth) {
        self.auth = Some(auth);
        self.nonce = Arc::default();
    }

    /// Every perp and spot market, or one market, with the fields [`InstrumentInfo`] drops.
    pub async fn get_order_book_details(
        &self,
        market_id: Option<u16>,
    ) -> InfraResult<RestOrderBookDetailsLighter> {
        let url = match market_id {
            Some(market_id) => format!(
                "{}{}?market_id={}",
                self.venue.base_url(),
                LIGHTER_ORDER_BOOK_DETAILS,
                market_id
            ),
            None => format!(
                "{}{}?filter=all",
                self.venue.base_url(),
                LIGHTER_ORDER_BOOK_DETAILS
            ),
        };

        let response = self.client.get(url).send().await?;
        let res: RestResLighter<RestOrderBookDetailsLighter> =
            parse_json_response("Lighter order_book_details", response).await?;

        res.into_one()
    }

    pub fn chain_id(&self) -> u32 {
        self.venue.chain_id()
    }

    fn auth_ref(&self) -> InfraResult<&LighterAuth> {
        self.auth.as_ref().ok_or(InfraError::ApiCliNotInitialized)
    }

    fn tx_header(&self, nonce: i64) -> InfraResult<TxHeader> {
        let auth = self.auth_ref()?;
        Ok(TxHeader {
            account_index: auth.account_index,
            api_key_index: auth.api_key_index,
            expired_at: get_mills_timestamp() as i64 + LIGHTER_TX_TTL_MS,
            nonce,
        })
    }

    fn sign<T: LighterTx>(&self, tx: T) -> InfraResult<SignedLighterTx> {
        self.auth_ref()?.sign_tx(tx, self.chain_id())
    }

    fn invalidate_on_err<T>(&self, res: InfraResult<T>) -> InfraResult<T> {
        if res.is_err() {
            self.nonce.invalidate();
        }
        res
    }

    /// Signs and sends one transaction built from a fresh header; returns its hash.
    async fn submit<T: LighterTx>(
        &self,
        build: impl Fn(TxHeader) -> InfraResult<T>,
    ) -> InfraResult<String> {
        let tx = self.sign_with_nonces(1, |_, h| build(h)).await?;
        Ok(self.invalidate_on_err(self.send_tx(&tx[0]).await)?.tx_hash)
    }

    /// Signs `n` transactions on consecutive nonces; a failed signing returns the nonces to the exchange count.
    async fn sign_with_nonces<T: LighterTx>(
        &self,
        n: usize,
        build: impl Fn(usize, TxHeader) -> InfraResult<T>,
    ) -> InfraResult<Vec<SignedLighterTx>> {
        let start = self.reserve_nonces(n as i64).await?;
        let signed = (0..n)
            .map(|i| self.sign(build(i, self.tx_header(start + i as i64)?)?))
            .collect();
        self.invalidate_on_err(signed)
    }

    async fn reserve_nonces(&self, n: i64) -> InfraResult<i64> {
        if let Some(start) = self.nonce.take(n) {
            return Ok(start);
        }
        let fresh = self.next_nonce().await?;
        Ok(self.nonce.seed_and_take(fresh, n))
    }

    /// Forgets the local nonce count, so the next signing re-reads it from the exchange. Needed after a
    /// transaction sent elsewhere (websocket) is rejected or lost: later nonces wait behind the gap.
    pub fn invalidate_nonce(&self) {
        self.nonce.invalidate();
    }

    /// Signed `CreateOrder`s on consecutive nonces, for `send_tx_batch` or the websocket.
    pub async fn sign_orders(&self, orders: &[OrderParams]) -> InfraResult<Vec<SignedLighterTx>> {
        let scales = orders
            .iter()
            .map(|p| self.market_scale(cli_to_lighter_market_id(&p.inst)?))
            .collect::<InfraResult<Vec<_>>>()?;
        let now_ms = get_mills_timestamp() as i64;
        self.sign_with_nonces(orders.len(), |i, h| {
            lighter_order_from_params(&orders[i], scales[i], h, now_ms, LIGHTER_ORDER_TTL_MS)
        })
        .await
    }

    /// Signed `CancelOrder`s (by exchange order id, else client order id) on consecutive nonces.
    pub async fn sign_cancels(
        &self,
        cancels: &[CancelOrderParams],
    ) -> InfraResult<Vec<SignedLighterTx>> {
        let targets = cancels
            .iter()
            .map(|c| {
                lighter_cancel_target(&c.inst, c.order_id.as_deref(), c.cli_order_id.as_deref())
            })
            .collect::<InfraResult<Vec<_>>>()?;
        self.sign_with_nonces(cancels.len(), |i, h| {
            let (market_index, index) = targets[i];
            Ok(LighterCancelOrderTx {
                account_index: h.account_index,
                api_key_index: h.api_key_index,
                market_index,
                index,
                expired_at: h.expired_at,
                nonce: h.nonce,
                ..Default::default()
            })
        })
        .await
    }

    /// Signed `ModifyOrder` moving a resting order to `size` at `price`.
    pub async fn sign_modify(
        &self,
        inst: &str,
        order_id: Option<&str>,
        cli_order_id: Option<&str>,
        size: &str,
        price: &str,
    ) -> InfraResult<SignedLighterTx> {
        let scale = self.market_scale(cli_to_lighter_market_id(inst)?)?;
        let mut tx = self
            .sign_with_nonces(1, |_, h| {
                lighter_modify_from_params(inst, order_id, cli_order_id, size, price, scale, h)
            })
            .await?;
        Ok(tx.remove(0))
    }

    /// Moves a resting order to `size` at `price` in one transaction; returns its hash.
    pub async fn modify_order(
        &self,
        inst: &str,
        order_id: Option<&str>,
        cli_order_id: Option<&str>,
        size: &str,
        price: &str,
    ) -> InfraResult<String> {
        let tx = self
            .sign_modify(inst, order_id, cli_order_id, size, price)
            .await?;
        Ok(self.invalidate_on_err(self.send_tx(&tx).await)?.tx_hash)
    }

    /// Orders by client order id (active ones, and inactive ones of the last 24 h), any market.
    pub async fn get_orders_by_client_ids(
        &self,
        cli_order_ids: &[i64],
    ) -> InfraResult<Vec<OrderDetailData>> {
        let account = self.auth_ref()?.account_index.to_string();
        let mut orders = Vec::with_capacity(cli_order_ids.len());
        for chunk in cli_order_ids.chunks(LIGHTER_ACCOUNT_ORDERS_MAX) {
            let ids = chunk
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let query = [
                ("account_index", account.clone()),
                ("client_order_indexes", ids),
            ];
            let res: RestOpenOrdersLighter = self
                .get_private(LIGHTER_ACCOUNT_ORDERS, &query, "Lighter accountOrders")
                .await?;
            orders.extend(res.orders.into_iter().map(|o| o.into_order_detail_data()));
        }
        Ok(orders)
    }

    /// Latest fills of the account, newest first, one market or all; at most 100.
    pub async fn get_fills(
        &self,
        inst: Option<&str>,
        limit: Option<u32>,
    ) -> InfraResult<Vec<LighterFill>> {
        let account = self.auth_ref()?.account_index;
        let mut query = vec![
            ("account_index", account.to_string()),
            ("sort_by", "timestamp".to_string()),
            ("sort_dir", "desc".to_string()),
            (
                "limit",
                limit
                    .unwrap_or(LIGHTER_HISTORY_PAGE_MAX)
                    .clamp(1, LIGHTER_HISTORY_PAGE_MAX)
                    .to_string(),
            ),
        ];
        if let Some(inst) = inst {
            query.push(("market_id", cli_to_lighter_market_id(inst)?.to_string()));
        }
        let res: RestTradesLighter = self
            .get_private(LIGHTER_TRADES, &query, "Lighter trades")
            .await?;
        Ok(res
            .trades
            .iter()
            .filter_map(|t| t.fill_of(account))
            .collect())
    }

    /// Tier (`standard` / `premium`) and fee ticks of the account.
    pub async fn get_account_limits(&self) -> InfraResult<RestAccountLimitsLighter> {
        let query = [("account_index", self.auth_ref()?.account_index.to_string())];
        self.get_private(LIGHTER_ACCOUNT_LIMITS, &query, "Lighter accountLimits")
            .await
    }

    /// Latest funding payments of the account, newest first, one market or all; at most 100.
    pub async fn get_position_funding(
        &self,
        inst: Option<&str>,
        limit: Option<u32>,
    ) -> InfraResult<Vec<PositionFundingLighter>> {
        let mut query = vec![
            ("account_index", self.auth_ref()?.account_index.to_string()),
            (
                "limit",
                limit
                    .unwrap_or(LIGHTER_HISTORY_PAGE_MAX)
                    .clamp(1, LIGHTER_HISTORY_PAGE_MAX)
                    .to_string(),
            ),
        ];
        if let Some(inst) = inst {
            query.push(("market_ids", cli_to_lighter_market_id(inst)?.to_string()));
        }
        let res: RestPositionFundingLighter = self
            .get_private(LIGHTER_POSITION_FUNDING, &query, "Lighter positionFunding")
            .await?;
        Ok(res.position_fundings)
    }

    /// GET with an auth token.
    async fn get_private<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, String)],
        ctx: &str,
    ) -> InfraResult<T> {
        let response = self
            .client
            .get(format!("{}{}", self.venue.base_url(), path))
            .query(query)
            .header("authorization", self.auth_token()?)
            .send()
            .await?;
        let res: RestResLighter<T> = parse_json_response(ctx, response).await?;
        res.into_one()
    }

    /// Next nonce of the API key according to the exchange.
    pub async fn next_nonce(&self) -> InfraResult<i64> {
        let auth = self.auth_ref()?;
        let url = format!(
            "{}{}?account_index={}&api_key_index={}",
            self.venue.base_url(),
            LIGHTER_NEXT_NONCE,
            auth.account_index,
            auth.api_key_index
        );
        let response = self.client.get(url).send().await?;
        let res: RestResLighter<RestNextNonceLighter> =
            parse_json_response("Lighter nextNonce", response).await?;
        Ok(res.into_one()?.nonce)
    }

    /// Auth token for private reads, signed per call (well under a millisecond).
    pub fn auth_token(&self) -> InfraResult<String> {
        let deadline = get_micros_timestamp() / 1_000_000 + LIGHTER_AUTH_TOKEN_TTL_S;
        self.auth_ref()?.auth_token(deadline)
    }

    /// Size and price decimals of every market of the venue; orders need them.
    pub async fn init_market_scales(&mut self) -> InfraResult<()> {
        self.market_scales = self
            .get_order_book_details(None)
            .await?
            .into_markets()
            .map(|m| {
                let scale = LighterMarketScale {
                    size_decimals: m.supported_size_decimals.max(0) as u32,
                    price_decimals: m.supported_price_decimals.max(0) as u32,
                };
                (m.market_id, scale)
            })
            .collect();
        Ok(())
    }

    pub fn market_scale(&self, market_id: u16) -> InfraResult<LighterMarketScale> {
        self.market_scales.get(&market_id).copied().ok_or_else(|| {
            InfraError::ApiCliError(if self.market_scales.is_empty() {
                "Lighter market scales are empty, call init_market_scales() first".into()
            } else {
                format!("Lighter market {market_id} not found")
            })
        })
    }

    /// Signed `CreateOrder` for one order with the given nonce.
    pub fn create_order_tx(&self, p: &OrderParams, nonce: i64) -> InfraResult<SignedLighterTx> {
        let scale = self.market_scale(cli_to_lighter_market_id(&p.inst)?)?;
        let tx = lighter_order_from_params(
            p,
            scale,
            self.tx_header(nonce)?,
            get_mills_timestamp() as i64,
            LIGHTER_ORDER_TTL_MS,
        )?;
        self.sign(tx)
    }

    pub async fn send_tx(&self, tx: &SignedLighterTx) -> InfraResult<RestSendTxLighter> {
        let url = format!("{}{}", self.venue.base_url(), LIGHTER_SEND_TX);
        let form = [
            ("tx_type", tx.tx_type.to_string()),
            ("tx_info", tx.tx_info.clone()),
        ];
        let response = self.client.post(url).form(&form).send().await?;
        let res: RestResLighter<RestSendTxLighter> =
            parse_json_response("Lighter sendTx", response).await?;
        res.into_one()
    }

    pub async fn send_tx_batch(
        &self,
        txs: &[SignedLighterTx],
    ) -> InfraResult<RestSendTxBatchLighter> {
        let url = format!("{}{}", self.venue.base_url(), LIGHTER_SEND_TX_BATCH);
        let types: Vec<u8> = txs.iter().map(|t| t.tx_type).collect();
        let infos: Vec<&str> = txs.iter().map(|t| t.tx_info.as_str()).collect();
        let form = [
            (
                "tx_types",
                serde_json::to_string(&types).unwrap_or_default(),
            ),
            (
                "tx_infos",
                serde_json::to_string(&infos).unwrap_or_default(),
            ),
        ];
        let response = self.client.post(url).form(&form).send().await?;
        let res: RestResLighter<RestSendTxBatchLighter> =
            parse_json_response("Lighter sendTxBatch", response).await?;
        res.into_one()
    }

    /// Leverage of one market (`initial margin fraction = 1 / leverage`); returns the transaction hash.
    pub async fn update_leverage(
        &self,
        inst: &str,
        leverage: u32,
        margin_mode: MarginMode,
    ) -> InfraResult<String> {
        let market_index = cli_to_lighter_market_id(inst)? as i16;
        if leverage == 0 {
            return Err(InfraError::ApiCliError(
                "Lighter leverage must be > 0".into(),
            ));
        }
        let margin_mode = match margin_mode {
            MarginMode::Cross => LIGHTER_CROSS_MARGIN,
            MarginMode::Isolated => LIGHTER_ISOLATED_MARGIN,
            MarginMode::Unknown => {
                return Err(InfraError::ApiCliError(
                    "Unknown Lighter margin mode".into(),
                ));
            },
        };
        let initial_margin_fraction = (10_000 / leverage) as u16;
        self.submit(|h| {
            Ok(LighterUpdateLeverageTx {
                account_index: h.account_index,
                api_key_index: h.api_key_index,
                market_index,
                initial_margin_fraction,
                margin_mode,
                expired_at: h.expired_at,
                nonce: h.nonce,
                ..Default::default()
            })
        })
        .await
    }

    /// Adds (or removes) isolated margin of one market, in USDC; returns the transaction hash.
    pub async fn update_margin(&self, inst: &str, usdc: f64, add: bool) -> InfraResult<String> {
        let market_index = cli_to_lighter_market_id(inst)? as i16;
        let usdc_amount = (usdc * 1e6).round() as i64;
        if usdc_amount <= 0 {
            return Err(InfraError::ApiCliError(format!(
                "Lighter margin amount {usdc} <= 0"
            )));
        }
        let direction = if add {
            LIGHTER_MARGIN_ADD
        } else {
            LIGHTER_MARGIN_REMOVE
        };
        self.submit(|h| {
            Ok(LighterUpdateMarginTx {
                account_index: h.account_index,
                api_key_index: h.api_key_index,
                market_index,
                usdc_amount,
                direction,
                expired_at: h.expired_at,
                nonce: h.nonce,
                ..Default::default()
            })
        })
        .await
    }

    /// Cancels every open order of the account now; returns the transaction hash.
    pub async fn cancel_all_orders(&self) -> InfraResult<String> {
        self.submit(|h| {
            Ok(LighterCancelAllOrdersTx {
                account_index: h.account_index,
                api_key_index: h.api_key_index,
                time_in_force: LIGHTER_CANCEL_ALL_IMMEDIATE,
                time: 0,
                expired_at: h.expired_at,
                nonce: h.nonce,
                ..Default::default()
            })
        })
        .await
    }

    pub async fn get_account(&self) -> InfraResult<AccountLighter> {
        let auth = self.auth_ref()?;
        let url = format!(
            "{}{}?by=index&value={}",
            self.venue.base_url(),
            LIGHTER_ACCOUNT,
            auth.account_index
        );
        let response = self.client.get(url).send().await?;
        let res: RestResLighter<RestAccountsLighter> =
            parse_json_response("Lighter account", response).await?;
        exactly_one(res.into_one()?.accounts)
    }

    /// The API key slot as registered on the exchange.
    pub async fn get_api_key(&self) -> InfraResult<ApiKeyLighter> {
        let auth = self.auth_ref()?;
        let url = format!(
            "{}{}?account_index={}&api_key_index={}",
            self.venue.base_url(),
            LIGHTER_API_KEYS,
            auth.account_index,
            auth.api_key_index
        );
        let response = self.client.get(url).send().await?;
        let res: RestResLighter<RestApiKeysLighter> =
            parse_json_response("Lighter apikeys", response).await?;
        exactly_one(res.into_one()?.api_keys)
    }

    /// Whether the local private key belongs to the registered public key of the slot.
    pub async fn check_api_key(&self) -> InfraResult<bool> {
        let local = encode_hex(&self.auth_ref()?.key().public_key()?);
        let registered = self.get_api_key().await?.public_key;
        Ok(registered
            .trim_start_matches("0x")
            .eq_ignore_ascii_case(&local))
    }

    async fn _get_tickers(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<TickerData>> {
        let timestamp = get_micros_timestamp();
        let data = self
            .get_order_book_details(None)
            .await?
            .into_markets()
            .filter(|m| inst_type.as_ref().is_none_or(|t| m.inst_type() == *t))
            .filter(|m| insts.is_none_or(|list| list.contains(&m.inst())))
            .map(|m| m.into_ticker_data(timestamp))
            .collect();

        Ok(data)
    }

    async fn _get_mark_prices(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<MarkPriceData>> {
        let timestamp = get_micros_timestamp();
        let data = self
            .get_order_book_details(None)
            .await?
            .into_markets()
            .filter(|m| inst_type.as_ref().is_none_or(|t| m.inst_type() == *t))
            .filter(|m| insts.is_none_or(|list| list.contains(&m.inst())))
            .filter_map(|m| m.into_mark_price_data(timestamp))
            .collect();

        Ok(data)
    }

    async fn _get_orderbook(
        &self,
        inst: &str,
        _inst_type: InstrumentType,
        depth: usize,
    ) -> InfraResult<OrderBookData> {
        let market_id = cli_to_lighter_market_id(inst)?;
        let url = format!(
            "{}{}?market_id={}&limit={}",
            self.venue.base_url(),
            LIGHTER_ORDER_BOOK_ORDERS,
            market_id,
            LIGHTER_ORDER_BOOK_ORDERS_LIMIT
        );

        let response = self.client.get(url).send().await?;
        let res: RestResLighter<RestOrderBookOrdersLighter> =
            parse_json_response("Lighter orderbook", response).await?;

        res.into_one()
            .map(|entry| entry.into_orderbook_data(inst, depth))
    }

    async fn _get_instrument_info(
        &self,
        inst_type: InstrumentType,
    ) -> InfraResult<Vec<InstrumentInfo>> {
        let data = self
            .get_order_book_details(None)
            .await?
            .into_markets()
            .map(InstrumentInfo::from)
            .filter(|i| i.inst_type == inst_type)
            .collect();

        Ok(data)
    }

    async fn _get_live_instruments(&self, inst_type: InstrumentType) -> InfraResult<Vec<String>> {
        let data = self
            ._get_instrument_info(inst_type)
            .await?
            .into_iter()
            .filter(|inst| inst.state == InstrumentStatus::Live)
            .map(|inst| inst.inst)
            .collect();

        Ok(data)
    }

    fn _get_public_sub_msg(
        &self,
        channel: &WsChannel,
        insts: Option<&[String]>,
    ) -> InfraResult<String> {
        let stream = match channel {
            WsChannel::Lob(lob_param) => lighter_lob_channel(lob_param)?,
            WsChannel::Trades(trades_param) => lighter_trades_channel(trades_param)?,
            WsChannel::Other(stream) => {
                let market_id = match insts {
                    Some(_) => Some(cli_to_lighter_market_id(single_ws_inst("Lighter", insts)?)?),
                    None => None,
                };
                return Ok(ws_subscribe_msg_lighter(stream, market_id));
            },
            _ => return Err(InfraError::Unimplemented),
        };

        let market_id = cli_to_lighter_market_id(single_ws_inst("Lighter", insts)?)?;

        Ok(ws_subscribe_msg_lighter(stream, Some(market_id)))
    }

    fn _get_private_sub_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        let name = lighter_private_channel(channel)?;
        Ok(ws_private_subscribe_msg_lighter(
            name,
            self.auth_ref()?.account_index,
            &self.auth_token()?,
        ))
    }

    fn _get_private_connect_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        match channel {
            WsChannel::AccountOrders | WsChannel::AccountPositions | WsChannel::Other(_) => {
                Ok(self.venue.ws_url().into())
            },
            _ => Err(InfraError::Unimplemented),
        }
    }

    fn _get_public_connect_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        match channel {
            WsChannel::Lob(_) | WsChannel::Trades(_) | WsChannel::Other(_) => {
                Ok(self.venue.ws_url().into())
            },
            _ => Err(InfraError::Unimplemented),
        }
    }
}

fn ack(status: OrderStatus, cli_order_id: Option<String>, tx_hash: String) -> OrderAckData {
    OrderAckData {
        timestamp: get_micros_timestamp(),
        order_status: status,
        order_id: String::new(),
        cli_order_id,
        msg: Some(tx_hash),
    }
}

fn padded(mut hashes: Vec<String>, n: usize) -> Vec<String> {
    hashes.resize(n, String::new());
    hashes
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::exchange::lighter::auth::tests::vectors;

    fn channel_of(msg: InfraResult<String>) -> String {
        serde_json::from_str::<Value>(&msg.unwrap()).unwrap()["channel"]
            .as_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn public_sub_msgs_follow_the_channel() {
        let cli = LighterCli::default();
        let nvda = vec!["@110".to_string()];

        assert_eq!(
            channel_of(cli._get_public_sub_msg(&WsChannel::Lob(None), Some(&nvda))),
            "order_book/110"
        );
        assert_eq!(
            channel_of(cli._get_public_sub_msg(
                &WsChannel::Lob(Some(LobParam::Bbo { frequency: None })),
                Some(&nvda)
            )),
            "ticker/110"
        );
        assert_eq!(
            channel_of(cli._get_public_sub_msg(&WsChannel::Trades(None), Some(&nvda))),
            "trade/110"
        );
        assert_eq!(
            channel_of(
                cli._get_public_sub_msg(&WsChannel::Other("market_stats".into()), Some(&nvda))
            ),
            "market_stats/110"
        );
        assert_eq!(
            channel_of(cli._get_public_sub_msg(&WsChannel::Other("market_stats/all".into()), None)),
            "market_stats/all"
        );
    }

    #[test]
    fn public_sub_msgs_reject_bad_instruments_and_channels() {
        let cli = LighterCli::default();

        assert!(
            cli._get_public_sub_msg(&WsChannel::Lob(None), None)
                .is_err()
        );
        assert!(
            cli._get_public_sub_msg(&WsChannel::Lob(None), Some(&["NVDA".to_string()]))
                .is_err()
        );
        assert!(matches!(
            cli._get_public_sub_msg(&WsChannel::AccountOrders, None),
            Err(InfraError::Unimplemented)
        ));
        assert!(matches!(
            cli._get_public_connect_msg(&WsChannel::AccountOrders),
            Err(InfraError::Unimplemented)
        ));
        assert_eq!(
            cli._get_public_connect_msg(&WsChannel::Trades(None))
                .unwrap(),
            LIGHTER_WS
        );
    }

    #[test]
    fn private_sub_msgs_sign_a_token_for_the_account() {
        let mut cli = LighterCli::default();
        assert!(matches!(
            cli._get_private_sub_msg(&WsChannel::AccountOrders),
            Err(InfraError::ApiCliNotInitialized)
        ));
        cli.set_auth(LighterAuth::new(758666, 4, &vectors().private_key).unwrap());

        let msg: Value =
            serde_json::from_str(&cli._get_private_sub_msg(&WsChannel::AccountOrders).unwrap())
                .unwrap();
        assert_eq!(msg["channel"], "account_all_orders/758666");
        let token = msg["auth"].as_str().unwrap();
        let deadline: u64 = token.split(':').next().unwrap().parse().unwrap();
        assert!(deadline > get_micros_timestamp() / 1_000_000);
        assert!(token[token.find(':').unwrap()..].starts_with(":758666:4:"));

        assert_eq!(
            channel_of(cli._get_private_sub_msg(&WsChannel::AccountPositions)),
            "account_all_positions/758666"
        );
        assert_eq!(
            channel_of(cli._get_private_sub_msg(&WsChannel::Other("account_all_trades".into()))),
            "account_all_trades/758666"
        );
        assert!(
            cli._get_private_sub_msg(&WsChannel::Other(LIGHTER_TX_CHANNEL.into()))
                .is_err()
        );
    }

    #[test]
    fn private_connections_use_the_venue_stream() {
        let mut cli = LighterCli::default();
        for channel in [
            WsChannel::AccountOrders,
            WsChannel::AccountPositions,
            WsChannel::Other(LIGHTER_TX_CHANNEL.into()),
        ] {
            assert_eq!(cli._get_private_connect_msg(&channel).unwrap(), LIGHTER_WS);
        }
        assert!(matches!(
            cli._get_private_connect_msg(&WsChannel::AccountBalAndPos),
            Err(InfraError::Unimplemented)
        ));
        cli.set_venue(LighterVenue::Robinhood);
        assert_eq!(
            cli._get_private_connect_msg(&WsChannel::AccountOrders)
                .unwrap(),
            LIGHTER_RH_WS
        );
    }

    #[test]
    fn each_venue_uses_its_own_endpoints() {
        let mut cli = LighterCli::default();
        assert_eq!(cli.venue.market(), LIGHTER);
        assert_eq!(cli.venue.base_url(), LIGHTER_BASE_URL);

        cli.set_venue(LighterVenue::Robinhood);
        assert_eq!(cli.venue.market(), LIGHTER_RH);
        assert_eq!(cli.venue.base_url(), LIGHTER_RH_BASE_URL);
        for channel in [
            WsChannel::Lob(None),
            WsChannel::Trades(None),
            WsChannel::Other("market_stats".into()),
        ] {
            assert_eq!(
                cli._get_public_connect_msg(&channel).unwrap(),
                LIGHTER_RH_WS
            );
        }
    }
}
