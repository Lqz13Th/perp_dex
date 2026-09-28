use reqwest::Client;
use std::{collections::HashSet, sync::Arc};

use extrema_infra::{
    arch::market_assets::{
        api_data::{price_data::*, utils_data::*},
        api_general::{get_micros_timestamp, parse_json_response},
    },
    prelude::*,
};

use super::{
    apex_rest_msg::RestResApex,
    api_utils::*,
    config_assets::*,
    schemas::rest::{
        orderbook::RestOrderBookApex, symbols::RestSymbolsApex, ticker::RestTickerApex,
    },
};

#[derive(Clone, Debug)]
pub struct ApexCli {
    pub client: Arc<Client>,
}

impl Default for ApexCli {
    fn default() -> Self {
        Self::new(Arc::new(Client::new()))
    }
}

impl LobPublicRest for ApexCli {
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

impl LobWebsocket for ApexCli {
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

impl ApexCli {
    pub fn new(shared_client: Arc<Client>) -> Self {
        Self {
            client: shared_client,
        }
    }

    /// Full `symbols` config: crypto, stock and prediction contracts with the fields [`InstrumentInfo`] drops.
    pub async fn get_symbols(&self) -> InfraResult<RestSymbolsApex> {
        let url = [APEX_BASE_URL, APEX_SYMBOLS].concat();

        let response = self.client.get(url).send().await?;
        let res: RestResApex<RestSymbolsApex> =
            parse_json_response("ApeX symbols", response).await?;

        res.into_one()
    }

    /// Every ticker ApeX lists, prediction contracts and retired symbols included.
    pub async fn get_all_tickers(&self) -> InfraResult<Vec<RestTickerApex>> {
        let url = [APEX_BASE_URL, APEX_ALL_TICKERS].concat();

        let response = self.client.get(url).send().await?;
        let res: RestResApex<RestTickerApex> =
            parse_json_response("ApeX all_ticker_info", response).await?;

        res.into_vec()
    }

    async fn perp_tickers(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<RestTickerApex>> {
        if inst_type.is_some_and(|t| t != InstrumentType::Perpetual) {
            return Ok(Vec::new());
        }

        let perps: HashSet<String> = self
            .get_symbols()
            .await?
            .contractConfig
            .into_perps()
            .map(|c| c.inst())
            .collect();
        let data = self
            .get_all_tickers()
            .await?
            .into_iter()
            .filter(|t| perps.contains(&t.inst()))
            .filter(|t| insts.is_none_or(|list| list.contains(&t.inst())))
            .collect();

        Ok(data)
    }

    async fn _get_tickers(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<TickerData>> {
        let timestamp = get_micros_timestamp();
        let data = self
            .perp_tickers(insts, inst_type)
            .await?
            .into_iter()
            .filter_map(|t| t.into_ticker_data(timestamp))
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
            .perp_tickers(insts, inst_type)
            .await?
            .into_iter()
            .filter_map(|t| t.into_mark_price_data(timestamp))
            .collect();

        Ok(data)
    }

    async fn _get_orderbook(
        &self,
        inst: &str,
        inst_type: InstrumentType,
        depth: usize,
    ) -> InfraResult<OrderBookData> {
        if inst_type != InstrumentType::Perpetual {
            return Err(InfraError::ApiCliError(format!(
                "ApeX orderbook supports perpetual instruments only, got {:?}",
                inst_type
            )));
        }

        let url = format!(
            "{}{}?symbol={}&limit={}",
            APEX_BASE_URL,
            APEX_DEPTH,
            cli_perp_to_apex_symbol(inst)?,
            apex_orderbook_limit(depth)?
        );

        let response = self.client.get(url).send().await?;
        let res: RestResApex<RestOrderBookApex> =
            parse_json_response("ApeX orderbook", response).await?;

        res.into_one()?
            .into_orderbook_data(inst, get_micros_timestamp())
    }

    async fn _get_instrument_info(
        &self,
        inst_type: InstrumentType,
    ) -> InfraResult<Vec<InstrumentInfo>> {
        let data = self
            .get_symbols()
            .await?
            .contractConfig
            .into_perps()
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
        let topic = match channel {
            WsChannel::Lob(lob_param) => apex_lob_topic(lob_param)?,
            WsChannel::Trades(trades_param) => apex_trades_topic(trades_param)?,
            WsChannel::Other(topic) => return ws_subscribe_msg_apex(topic, insts),
            _ => return Err(InfraError::Unimplemented),
        };

        if insts.is_none_or(<[String]>::is_empty) {
            return Err(InfraError::ApiCliError(
                "ApeX ws requires at least one instrument".into(),
            ));
        }

        ws_subscribe_msg_apex(topic, insts)
    }

    fn _get_public_connect_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        match channel {
            WsChannel::Lob(_) | WsChannel::Trades(_) | WsChannel::Other(_) => Ok(APEX_WS.into()),
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

    #[test]
    fn public_sub_msgs_follow_the_channel() {
        let cli = ApexCli::default();
        let nvda = vec!["NVDA_USDT_PERP".to_string()];

        assert_eq!(
            msg(cli._get_public_sub_msg(&WsChannel::Lob(None), Some(&nvda))),
            json!({"op":"subscribe","args":["orderBook200.H.NVDAUSDT"]})
        );
        assert_eq!(
            msg(cli._get_public_sub_msg(
                &WsChannel::Lob(Some(LobParam::Incremental {
                    depth: Some(25),
                    frequency: None
                })),
                Some(&nvda)
            )),
            json!({"op":"subscribe","args":["orderBook25.H.NVDAUSDT"]})
        );
        assert_eq!(
            msg(cli._get_public_sub_msg(&WsChannel::Trades(None), Some(&nvda))),
            json!({"op":"subscribe","args":["recentlyTrade.H.NVDAUSDT"]})
        );
        assert_eq!(
            msg(cli._get_public_sub_msg(&WsChannel::Other("instrumentInfo.H".into()), Some(&nvda))),
            json!({"op":"subscribe","args":["instrumentInfo.H.NVDAUSDT"]})
        );
        assert_eq!(
            msg(cli._get_public_sub_msg(&WsChannel::Other("instrumentInfo.all".into()), None)),
            json!({"op":"subscribe","args":["instrumentInfo.all"]})
        );
    }

    #[test]
    fn public_sub_msgs_reject_bad_instruments_and_channels() {
        let cli = ApexCli::default();

        assert!(
            cli._get_public_sub_msg(&WsChannel::Lob(None), None)
                .is_err()
        );
        assert!(
            cli._get_public_sub_msg(&WsChannel::Trades(None), Some(&[]))
                .is_err()
        );
        assert!(
            cli._get_public_sub_msg(&WsChannel::Lob(None), Some(&["NVDAUSDT".to_string()]))
                .is_err()
        );
        assert!(matches!(
            cli._get_public_sub_msg(
                &WsChannel::Lob(Some(LobParam::Bbo { frequency: None })),
                Some(&["NVDA_USDT_PERP".to_string()])
            ),
            Err(InfraError::ApiCliError(_))
        ));
        assert!(matches!(
            cli._get_public_sub_msg(&WsChannel::AccountOrders, None),
            Err(InfraError::Unimplemented)
        ));
        assert!(matches!(
            cli._get_public_sub_msg(&WsChannel::Candles(None), None),
            Err(InfraError::Unimplemented)
        ));
        assert!(matches!(
            cli._get_public_connect_msg(&WsChannel::AccountPositions),
            Err(InfraError::Unimplemented)
        ));
        assert_eq!(
            cli._get_public_connect_msg(&WsChannel::Lob(None)).unwrap(),
            APEX_WS
        );
    }
}
