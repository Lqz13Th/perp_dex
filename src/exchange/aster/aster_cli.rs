use reqwest::Client;
use std::sync::Arc;

use extrema_infra::{
    arch::market_assets::{
        api_data::{price_data::*, utils_data::*},
        api_general::parse_json_response,
    },
    prelude::*,
};

use super::{
    api_utils::*,
    aster_rest_msg::RestResAster,
    config_assets::*,
    schemas::rest::{
        exchange_info::RestExchangeInfoAster, funding_info::RestFundingInfoAster,
        orderbook::RestOrderBookAster, premium_index::RestPremiumIndexAster,
        ticker::RestTickerAster,
    },
};

#[derive(Clone, Debug)]
pub struct AsterCli {
    pub client: Arc<Client>,
}

impl Default for AsterCli {
    fn default() -> Self {
        Self::new(Arc::new(Client::new()))
    }
}

impl LobPublicRest for AsterCli {
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

impl LobWebsocket for AsterCli {
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

impl AsterCli {
    pub fn new(shared_client: Arc<Client>) -> Self {
        Self {
            client: shared_client,
        }
    }

    /// Full `exchangeInfo`, including the stock classification that [`InstrumentInfo`] drops.
    pub async fn get_exchange_info(&self) -> InfraResult<RestExchangeInfoAster> {
        let url = [ASTER_BASE_URL, ASTER_EXCHANGE_INFO].concat();

        let response = self.client.get(url).send().await?;
        let res: RestResAster<RestExchangeInfoAster> =
            parse_json_response("Aster exchange_info", response).await?;

        res.into_one()
    }

    pub async fn get_premium_index(
        &self,
        inst: Option<&str>,
    ) -> InfraResult<Vec<RestPremiumIndexAster>> {
        let mut url = [ASTER_BASE_URL, ASTER_PREMIUM_INDEX].concat();

        if let Some(inst) = inst {
            url.push_str(&format!("?symbol={}", cli_perp_to_aster_symbol(inst)));
        }

        let response = self.client.get(url).send().await?;
        let res: RestResAster<RestPremiumIndexAster> =
            parse_json_response("Aster premium_index", response).await?;

        res.into_vec()
    }

    pub async fn get_funding_rate_live(
        &self,
        inst: Option<&str>,
    ) -> InfraResult<Vec<FundingRateData>> {
        let data = self
            .get_premium_index(inst)
            .await?
            .into_iter()
            .map(FundingRateData::from)
            .collect();

        Ok(data)
    }

    pub async fn get_funding_info(&self) -> InfraResult<Vec<FundingRateInfo>> {
        let url = [ASTER_BASE_URL, ASTER_FUNDING_INFO].concat();

        let response = self.client.get(url).send().await?;
        let res: RestResAster<RestFundingInfoAster> =
            parse_json_response("Aster funding_info", response).await?;

        let data = res
            .into_vec()?
            .into_iter()
            .map(FundingRateInfo::from)
            .collect();

        Ok(data)
    }

    async fn _get_tickers(
        &self,
        insts: Option<&[String]>,
        _inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<TickerData>> {
        let url = [ASTER_BASE_URL, ASTER_TICKER_PRICE].concat();

        let response = self.client.get(url).send().await?;
        let res: RestResAster<RestTickerAster> =
            parse_json_response("Aster tickers", response).await?;

        let data = res
            .into_vec()?
            .into_iter()
            .map(TickerData::from)
            .filter(|t| match insts {
                Some(list) => list.contains(&t.inst),
                None => true,
            })
            .collect();

        Ok(data)
    }

    async fn _get_mark_prices(
        &self,
        insts: Option<&[String]>,
        _inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<MarkPriceData>> {
        let data = self
            .get_premium_index(None)
            .await?
            .into_iter()
            .map(MarkPriceData::from)
            .filter(|p| match insts {
                Some(list) => list.contains(&p.inst),
                None => true,
            })
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
                "Aster orderbook supports perpetual instruments only, got {:?}",
                inst_type
            )));
        }

        let mut params = vec![format!("symbol={}", cli_perp_to_aster_symbol(inst))];
        if depth > 0 {
            params.push(format!("limit={}", aster_orderbook_limit(depth)?));
        }

        let url = format!("{}{}?{}", ASTER_BASE_URL, ASTER_DEPTH, params.join("&"));
        let response = self.client.get(url).send().await?;
        let res: RestResAster<RestOrderBookAster> =
            parse_json_response("Aster orderbook", response).await?;

        res.into_one().map(|entry| entry.into_orderbook_data(inst))
    }

    async fn _get_instrument_info(
        &self,
        inst_type: InstrumentType,
    ) -> InfraResult<Vec<InstrumentInfo>> {
        let data = self
            .get_exchange_info()
            .await?
            .symbols
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
        ws_channel: &WsChannel,
        insts: Option<&[String]>,
    ) -> InfraResult<String> {
        match ws_channel {
            WsChannel::Trades(trades_param) => Ok(ws_subscribe_msg_aster(
                aster_trades_stream(trades_param),
                insts,
            )),
            WsChannel::Lob(lob_param) => {
                Ok(ws_subscribe_msg_aster(&aster_lob_stream(lob_param)?, insts))
            },
            WsChannel::Other(stream) => Ok(ws_subscribe_msg_aster(stream, insts)),
            _ => Err(InfraError::Unimplemented),
        }
    }

    fn _get_public_connect_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        match channel {
            WsChannel::Trades(_) | WsChannel::Lob(_) | WsChannel::Other(_) => Ok(ASTER_WS.into()),
            _ => Err(InfraError::Unimplemented),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn sub_params(msg: InfraResult<String>) -> Value {
        serde_json::from_str::<Value>(&msg.unwrap()).unwrap()["params"].clone()
    }

    #[test]
    fn public_sub_msgs_follow_the_channel() {
        let cli = AsterCli::default();
        let insts = vec!["NVDA_USDT_PERP".to_string()];

        assert_eq!(
            sub_params(cli._get_public_sub_msg(&WsChannel::Trades(None), Some(&insts))),
            json!(["nvdausdt@aggTrade"])
        );
        assert_eq!(
            sub_params(cli._get_public_sub_msg(
                &WsChannel::Trades(Some(TradesParam::AllTrades)),
                Some(&insts)
            )),
            json!(["nvdausdt@trade"])
        );
        assert_eq!(
            sub_params(cli._get_public_sub_msg(
                &WsChannel::Lob(Some(LobParam::Bbo { frequency: None })),
                Some(&insts)
            )),
            json!(["nvdausdt@bookTicker"])
        );
        assert_eq!(
            sub_params(
                cli._get_public_sub_msg(&WsChannel::Other("markPrice@1s".into()), Some(&insts))
            ),
            json!(["nvdausdt@markPrice@1s"])
        );
    }

    #[test]
    fn unsupported_channels_are_unimplemented() {
        let cli = AsterCli::default();

        assert!(matches!(
            cli._get_public_sub_msg(&WsChannel::AccountOrders, None),
            Err(InfraError::Unimplemented)
        ));
        assert!(matches!(
            cli._get_public_sub_msg(&WsChannel::Candles(None), None),
            Err(InfraError::Unimplemented)
        ));
        assert!(matches!(
            cli._get_public_connect_msg(&WsChannel::AccountOrders),
            Err(InfraError::Unimplemented)
        ));
        assert_eq!(
            cli._get_public_connect_msg(&WsChannel::Lob(None)).unwrap(),
            ASTER_WS
        );
    }
}
