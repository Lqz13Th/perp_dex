use reqwest::Client;
use std::sync::Arc;

use extrema_infra::{
    arch::market_assets::{
        api_data::{price_data::*, utils_data::*},
        api_general::parse_json_response,
    },
    prelude::*,
};

use crate::exchange::api_general::single_ws_inst;

use super::{
    api_utils::*,
    config_assets::*,
    pacifica_rest_msg::RestResPacifica,
    schemas::rest::{
        market_info::MarketInfoPacifica, orderbook::RestBookPacifica, prices::RestPricePacifica,
    },
};

#[derive(Clone, Debug)]
pub struct PacificaCli {
    pub client: Arc<Client>,
}

impl Default for PacificaCli {
    fn default() -> Self {
        Self::new(Arc::new(Client::new()))
    }
}

impl LobPublicRest for PacificaCli {
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

impl LobWebsocket for PacificaCli {
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

impl PacificaCli {
    pub fn new(shared_client: Arc<Client>) -> Self {
        Self {
            client: shared_client,
        }
    }

    /// Every perp and spot market with the fields [`InstrumentInfo`] drops. The API
    /// has no asset category, so stocks cannot be told apart from crypto here.
    pub async fn get_market_info(&self) -> InfraResult<Vec<MarketInfoPacifica>> {
        let url = [PACIFICA_BASE_URL, PACIFICA_INFO].concat();

        let response = self.client.get(url).send().await?;
        let res: RestResPacifica<MarketInfoPacifica> =
            parse_json_response("Pacifica info", response).await?;

        res.into_vec()
    }

    /// Mark, mid, oracle and funding for every market.
    pub async fn get_prices(&self) -> InfraResult<Vec<RestPricePacifica>> {
        let url = [PACIFICA_BASE_URL, PACIFICA_PRICES].concat();

        let response = self.client.get(url).send().await?;
        let res: RestResPacifica<RestPricePacifica> =
            parse_json_response("Pacifica prices", response).await?;

        res.into_vec()
    }

    async fn _get_tickers(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<TickerData>> {
        let data = self
            .get_prices()
            .await?
            .into_iter()
            .filter(|p| inst_type.as_ref().is_none_or(|t| p.inst_type() == *t))
            .filter(|p| insts.is_none_or(|list| list.contains(&p.inst())))
            .map(TickerData::from)
            .collect();

        Ok(data)
    }

    async fn _get_mark_prices(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<MarkPriceData>> {
        let data = self
            .get_prices()
            .await?
            .into_iter()
            .filter(|p| inst_type.as_ref().is_none_or(|t| p.inst_type() == *t))
            .filter(|p| insts.is_none_or(|list| list.contains(&p.inst())))
            .map(MarkPriceData::from)
            .collect();

        Ok(data)
    }

    async fn _get_orderbook(
        &self,
        inst: &str,
        inst_type: InstrumentType,
        depth: usize,
    ) -> InfraResult<OrderBookData> {
        let symbol = cli_to_pacifica_symbol(inst)?;
        if pacifica_inst_type(&symbol) != inst_type {
            return Err(InfraError::ApiCliError(format!(
                "Pacifica {inst} is not a {:?} instrument",
                inst_type
            )));
        }
        let depth = pacifica_orderbook_levels(depth)?;

        let url = format!(
            "{}{}?symbol={}&agg_level={}",
            PACIFICA_BASE_URL, PACIFICA_BOOK, symbol, PACIFICA_BOOK_AGG_LEVEL
        );
        let response = self.client.get(url).send().await?;
        let res: RestResPacifica<RestBookPacifica> =
            parse_json_response("Pacifica orderbook", response).await?;

        res.into_one()
            .map(|entry| entry.into_orderbook_data(inst, depth))
    }

    async fn _get_instrument_info(
        &self,
        inst_type: InstrumentType,
    ) -> InfraResult<Vec<InstrumentInfo>> {
        let data = self
            .get_market_info()
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
        let (source, agg_level) = match channel {
            WsChannel::Lob(lob_param) => pacifica_lob_source(lob_param)?,
            WsChannel::Trades(trades_param) => (pacifica_trades_source(trades_param)?, None),
            WsChannel::Other(source) => {
                let symbol = match insts {
                    Some(_) => Some(cli_to_pacifica_symbol(single_ws_inst("Pacifica", insts)?)?),
                    None => None,
                };
                return Ok(ws_subscribe_msg_pacifica(source, symbol.as_deref(), None));
            },
            _ => return Err(InfraError::Unimplemented),
        };

        let symbol = cli_to_pacifica_symbol(single_ws_inst("Pacifica", insts)?)?;

        Ok(ws_subscribe_msg_pacifica(source, Some(&symbol), agg_level))
    }

    fn _get_public_connect_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        match channel {
            WsChannel::Lob(_) | WsChannel::Trades(_) | WsChannel::Other(_) => {
                Ok(PACIFICA_WS.into())
            },
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
        let cli = PacificaCli::default();
        let btc = vec!["BTC_USDC_PERP".to_string()];
        let spot = vec!["SOL_USDC".to_string()];

        assert_eq!(
            msg(cli._get_public_sub_msg(&WsChannel::Lob(None), Some(&btc))),
            json!({"method":"subscribe","params":{"source":"book","symbol":"BTC","agg_level":1}})
        );
        assert_eq!(
            msg(cli._get_public_sub_msg(
                &WsChannel::Lob(Some(LobParam::Snapshot {
                    depth: Some(10),
                    frequency: None
                })),
                Some(&btc)
            )),
            json!({"method":"subscribe","params":{"source":"book","symbol":"BTC","agg_level":1}})
        );
        assert_eq!(
            msg(cli._get_public_sub_msg(
                &WsChannel::Lob(Some(LobParam::Bbo { frequency: None })),
                Some(&spot)
            )),
            json!({"method":"subscribe","params":{"source":"bbo","symbol":"SOL-USDC"}})
        );
        assert_eq!(
            msg(cli._get_public_sub_msg(&WsChannel::Trades(None), Some(&btc))),
            json!({"method":"subscribe","params":{"source":"trades","symbol":"BTC"}})
        );
        assert_eq!(
            msg(cli._get_public_sub_msg(&WsChannel::Other("prices".into()), None)),
            json!({"method":"subscribe","params":{"source":"prices"}})
        );
        assert_eq!(
            msg(cli._get_public_sub_msg(&WsChannel::Other("candle".into()), Some(&btc))),
            json!({"method":"subscribe","params":{"source":"candle","symbol":"BTC"}})
        );
    }

    #[test]
    fn public_sub_msgs_reject_bad_instruments_and_channels() {
        let cli = PacificaCli::default();

        assert!(
            cli._get_public_sub_msg(&WsChannel::Lob(None), None)
                .is_err()
        );
        assert!(
            cli._get_public_sub_msg(&WsChannel::Trades(None), Some(&["BTC".to_string()]))
                .is_err()
        );
        assert!(
            cli._get_public_sub_msg(&WsChannel::Lob(None), Some(&["NVDA_USDT_PERP".to_string()]))
                .is_err()
        );
        assert!(
            cli._get_public_sub_msg(
                &WsChannel::Lob(Some(LobParam::Incremental {
                    depth: None,
                    frequency: None
                })),
                Some(&["BTC_USDC_PERP".to_string()])
            )
            .is_err()
        );
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
            cli._get_public_connect_msg(&WsChannel::Trades(None))
                .unwrap(),
            PACIFICA_WS
        );
    }

    #[tokio::test]
    async fn orderbook_rejects_bad_requests_before_any_call() {
        let cli = PacificaCli::default();

        for (inst, inst_type, depth) in [
            ("BTC_USDC_PERP", InstrumentType::Spot, 5),
            ("SOL_USDC", InstrumentType::Perpetual, 5),
            ("BTC_USDC_PERP", InstrumentType::Futures, 5),
            ("BTC_USDC_PERP", InstrumentType::Perpetual, 11),
            ("BTC-USDC", InstrumentType::Perpetual, 5),
        ] {
            assert!(
                matches!(
                    cli._get_orderbook(inst, inst_type.clone(), depth).await,
                    Err(InfraError::ApiCliError(_))
                ),
                "{inst} {inst_type:?} {depth}"
            );
        }
    }
}
