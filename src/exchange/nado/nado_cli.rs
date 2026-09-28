use reqwest::Client;
use std::{collections::HashMap, sync::Arc};

use extrema_infra::{
    arch::market_assets::{
        api_data::{price_data::*, utils_data::*},
        api_general::{get_micros_timestamp, parse_json_response},
    },
    prelude::*,
};

use crate::exchange::api_general::single_ws_inst;

use super::{
    api_utils::*,
    config_assets::*,
    nado_rest_msg::RestResNado,
    schemas::rest::{
        contracts::RestContractNado,
        market_liquidity::RestMarketLiquidityNado,
        symbols::{RestSymbolsNado, SymbolNado},
        tickers::RestTickerNado,
    },
};

#[derive(Clone, Debug)]
pub struct NadoCli {
    pub client: Arc<Client>,
}

impl Default for NadoCli {
    fn default() -> Self {
        Self::new(Arc::new(Client::new()))
    }
}

impl LobPublicRest for NadoCli {
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

impl LobWebsocket for NadoCli {
    async fn get_public_sub_msg(
        &self,
        channel: &WsChannel,
        insts: Option<&[String]>,
    ) -> InfraResult<String> {
        self._get_public_sub_msg(channel, insts)
    }

    async fn get_public_connect_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        self._get_public_connect_msg(channel)
    }
}

impl NadoCli {
    pub fn new(shared_client: Arc<Client>) -> Self {
        Self {
            client: shared_client,
        }
    }

    /// Every spot and perp product, with the fees, margin weights and open-interest cap that [`InstrumentInfo`] drops.
    pub async fn get_symbols(&self) -> InfraResult<Vec<SymbolNado>> {
        let url = format!("{}{}?type=symbols", NADO_BASE_URL, NADO_GATEWAY_QUERY);

        let response = self.client.get(url).send().await?;
        let res: RestResNado<RestSymbolsNado> =
            parse_json_response("Nado symbols", response).await?;

        res.into_one().map(RestSymbolsNado::into_products)
    }

    /// The book with its nanosecond `timestamp`, the resync point for `book_depth` diffs.
    pub async fn get_market_liquidity(
        &self,
        inst: &str,
        depth: usize,
    ) -> InfraResult<RestMarketLiquidityNado> {
        let url = format!(
            "{}{}?type=market_liquidity&product_id={}&depth={}",
            NADO_BASE_URL,
            NADO_GATEWAY_QUERY,
            cli_to_nado_product_id(inst)?,
            nado_orderbook_depth(depth)?
        );

        let response = self.client.get(url).send().await?;
        let res: RestResNado<RestMarketLiquidityNado> =
            parse_json_response("Nado market_liquidity", response).await?;

        res.into_one()
    }

    async fn _get_tickers(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<TickerData>> {
        let inst_types = match inst_type {
            Some(inst_type) => vec![inst_type],
            None => vec![InstrumentType::Perpetual, InstrumentType::Spot],
        };
        let timestamp = get_micros_timestamp();
        let mut data = Vec::new();

        for inst_type in inst_types {
            let Some(market) = nado_ticker_market(&inst_type) else {
                continue;
            };
            let url = format!(
                "{}{}?market={}",
                NADO_BASE_URL, NADO_ARCHIVE_TICKERS, market
            );

            let response = self.client.get(url).send().await?;
            let res: RestResNado<HashMap<String, RestTickerNado>> =
                parse_json_response("Nado tickers", response).await?;

            data.extend(
                res.into_one()?
                    .into_values()
                    .filter(|t| insts.is_none_or(|list| list.contains(&t.inst())))
                    .filter_map(|t| t.into_ticker_data(inst_type.clone(), timestamp)),
            );
        }

        Ok(data)
    }

    async fn _get_mark_prices(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<MarkPriceData>> {
        if inst_type
            .as_ref()
            .is_some_and(|t| *t != InstrumentType::Perpetual)
        {
            return Ok(Vec::new());
        }

        let url = [NADO_BASE_URL, NADO_ARCHIVE_CONTRACTS].concat();
        let timestamp = get_micros_timestamp();

        let response = self.client.get(url).send().await?;
        let res: RestResNado<HashMap<String, RestContractNado>> =
            parse_json_response("Nado contracts", response).await?;

        let data = res
            .into_one()?
            .into_values()
            .filter(|c| insts.is_none_or(|list| list.contains(&c.inst())))
            .map(|c| c.into_mark_price_data(timestamp))
            .collect();

        Ok(data)
    }

    async fn _get_orderbook(
        &self,
        inst: &str,
        inst_type: InstrumentType,
        depth: usize,
    ) -> InfraResult<OrderBookData> {
        if !matches!(inst_type, InstrumentType::Perpetual | InstrumentType::Spot) {
            return Err(InfraError::ApiCliError(format!(
                "Nado orderbook supports perpetual and spot instruments only, got {:?}",
                inst_type
            )));
        }

        self.get_market_liquidity(inst, depth)
            .await
            .map(RestMarketLiquidityNado::into_orderbook_data)
    }

    async fn _get_instrument_info(
        &self,
        inst_type: InstrumentType,
    ) -> InfraResult<Vec<InstrumentInfo>> {
        let data = self
            .get_symbols()
            .await?
            .into_iter()
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
            WsChannel::Lob(lob_param) => nado_lob_stream(lob_param)?,
            WsChannel::Trades(trades_param) => nado_trades_stream(trades_param)?,
            WsChannel::Other(stream) => {
                let product_id = match insts {
                    Some(_) => Some(cli_to_nado_product_id(single_ws_inst("Nado", insts)?)?),
                    None => None,
                };
                return Ok(ws_subscribe_msg_nado(stream, product_id));
            },
            _ => return Err(InfraError::Unimplemented),
        };

        let product_id = cli_to_nado_product_id(single_ws_inst("Nado", insts)?)?;

        Ok(ws_subscribe_msg_nado(stream, Some(product_id)))
    }

    fn _get_public_connect_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        match channel {
            WsChannel::Lob(_) | WsChannel::Trades(_) | WsChannel::Other(_) => Ok(NADO_WS.into()),
            _ => Err(InfraError::Unimplemented),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn msg(res: InfraResult<String>) -> Value {
        serde_json::from_str(&res.unwrap()).unwrap()
    }

    fn sub(stream: Value) -> Value {
        json!({"method":"subscribe","stream":stream,"id":1})
    }

    #[test]
    fn public_sub_msgs_follow_the_channel() {
        let cli = NadoCli::default();
        let btc = vec!["@2".to_string()];

        assert_eq!(
            msg(cli._get_public_sub_msg(&WsChannel::Lob(None), Some(&btc))),
            sub(json!({"type":"book_depth","product_id":2}))
        );
        assert_eq!(
            msg(cli._get_public_sub_msg(
                &WsChannel::Lob(Some(LobParam::Bbo { frequency: None })),
                Some(&btc)
            )),
            sub(json!({"type":"best_bid_offer","product_id":2}))
        );
        assert_eq!(
            msg(cli
                ._get_public_sub_msg(&WsChannel::Trades(Some(TradesParam::AllTrades)), Some(&btc))),
            sub(json!({"type":"trade","product_id":2}))
        );
        assert_eq!(
            msg(cli._get_public_sub_msg(&WsChannel::Other("funding_rate".into()), Some(&btc))),
            sub(json!({"type":"funding_rate","product_id":2}))
        );
        assert_eq!(
            msg(cli._get_public_sub_msg(&WsChannel::Other("all_bbo".into()), None)),
            sub(json!({"type":"all_bbo"}))
        );
    }

    #[test]
    fn one_product_per_subscription() {
        let cli = NadoCli::default();
        let two = vec!["@112".to_string(), "@2".to_string()];

        assert_eq!(
            msg(cli._get_public_sub_msg(&WsChannel::Trades(None), Some(&two))),
            sub(json!({"type":"trade","product_id":112}))
        );
    }

    #[test]
    fn public_sub_msgs_reject_bad_instruments_and_channels() {
        let cli = NadoCli::default();

        assert!(
            cli._get_public_sub_msg(&WsChannel::Lob(None), None)
                .is_err()
        );
        assert!(
            cli._get_public_sub_msg(&WsChannel::Lob(None), Some(&["BTC-PERP".to_string()]))
                .is_err()
        );
        assert!(matches!(
            cli._get_public_sub_msg(
                &WsChannel::Lob(Some(LobParam::Snapshot {
                    depth: Some(5),
                    frequency: None
                })),
                Some(&["@2".to_string()])
            ),
            Err(InfraError::ApiCliError(_))
        ));
        assert!(matches!(
            cli._get_public_sub_msg(
                &WsChannel::Trades(Some(TradesParam::AggTrades)),
                Some(&["@2".to_string()])
            ),
            Err(InfraError::ApiCliError(_))
        ));
        for channel in [
            WsChannel::AccountOrders,
            WsChannel::Candles(None),
            WsChannel::LobMbo,
        ] {
            assert!(matches!(
                cli._get_public_sub_msg(&channel, Some(&["@2".to_string()])),
                Err(InfraError::Unimplemented)
            ));
        }
    }

    #[test]
    fn public_connect_msg_is_the_direct_subscription_endpoint() {
        let cli = NadoCli::default();

        for channel in [
            WsChannel::Lob(None),
            WsChannel::Trades(None),
            WsChannel::Other("all_bbo".into()),
        ] {
            assert_eq!(cli._get_public_connect_msg(&channel).unwrap(), NADO_WS);
        }
        assert!(matches!(
            cli._get_public_connect_msg(&WsChannel::AccountPositions),
            Err(InfraError::Unimplemented)
        ));
    }

    #[tokio::test]
    async fn orderbook_rejects_bad_requests_before_sending() {
        let cli = NadoCli::default();

        for (inst, inst_type, depth) in [
            ("@2", InstrumentType::Futures, 5),
            ("@2", InstrumentType::Perpetual, 101),
            ("BTC-PERP", InstrumentType::Perpetual, 5),
        ] {
            assert!(matches!(
                cli.get_orderbook(inst, inst_type, depth).await,
                Err(InfraError::ApiCliError(_))
            ));
        }
    }

    #[tokio::test]
    async fn unsupported_inst_types_have_no_prices() {
        let cli = NadoCli::default();

        assert!(
            cli.get_tickers(None, Some(InstrumentType::Futures))
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            cli.get_mark_prices(None, Some(InstrumentType::Spot))
                .await
                .unwrap()
                .is_empty()
        );
    }
}
