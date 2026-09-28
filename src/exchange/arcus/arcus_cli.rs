use reqwest::Client;
use std::sync::Arc;

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
    arcus_rest_msg::RestResArcus,
    config_assets::*,
    schemas::rest::{
        markets::{MarketArcus, RestMarketsArcus},
        orderbook::RestOrderBookArcus,
    },
};

#[derive(Clone, Debug)]
pub struct ArcusCli {
    pub client: Arc<Client>,
}

impl Default for ArcusCli {
    fn default() -> Self {
        Self::new(Arc::new(Client::new()))
    }
}

impl LobPublicRest for ArcusCli {
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

impl LobWebsocket for ArcusCli {
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

impl ArcusCli {
    pub fn new(shared_client: Arc<Client>) -> Self {
        Self {
            client: shared_client,
        }
    }

    /// Every market, with the category, trading hours and price bands that [`InstrumentInfo`] drops.
    pub async fn get_markets(&self) -> InfraResult<Vec<MarketArcus>> {
        let url = [ARCUS_BASE_URL, ARCUS_MARKETS].concat();

        let response = self.client.get(url).send().await?;
        let res: RestResArcus<RestMarketsArcus> =
            parse_json_response("Arcus markets", response).await?;

        res.into_one().map(|data| data.markets)
    }

    async fn _get_tickers(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<TickerData>> {
        let timestamp = get_micros_timestamp();
        let data = self
            .get_markets()
            .await?
            .into_iter()
            .filter(|m| inst_type.as_ref().is_none_or(|t| m.inst_type() == *t))
            .filter(|m| insts.is_none_or(|list| list.contains(&m.inst())))
            .filter_map(|m| m.into_ticker_data(timestamp))
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
            .get_markets()
            .await?
            .into_iter()
            .filter(|m| inst_type.as_ref().is_none_or(|t| m.inst_type() == *t))
            .filter(|m| insts.is_none_or(|list| list.contains(&m.inst())))
            .filter_map(|m| m.into_mark_price_data(timestamp))
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
                "Arcus orderbook supports perpetual instruments only, got {:?}",
                inst_type
            )));
        }

        let url = format!(
            "{}{}/{}?nLevels={}",
            ARCUS_BASE_URL,
            ARCUS_L2_ORDER_BOOK,
            cli_perp_to_arcus_market(inst)?,
            arcus_orderbook_levels(depth)?
        );

        let response = self.client.get(url).send().await?;
        let res: RestResArcus<RestOrderBookArcus> =
            parse_json_response("Arcus orderbook", response).await?;

        res.into_one().map(|entry| entry.into_orderbook_data(inst))
    }

    async fn _get_instrument_info(
        &self,
        inst_type: InstrumentType,
    ) -> InfraResult<Vec<InstrumentInfo>> {
        let data = self
            .get_markets()
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
        let (stream, n_levels) = match channel {
            WsChannel::Lob(lob_param) => arcus_lob_channel(lob_param)?,
            WsChannel::Trades(trades_param) => (arcus_trades_channel(trades_param)?, None),
            WsChannel::Other(stream) => {
                let market = match insts {
                    Some(_) => Some(cli_perp_to_arcus_market(single_ws_inst("Arcus", insts)?)?),
                    None => None,
                };
                return Ok(ws_subscribe_msg_arcus(stream, market.as_deref(), None));
            },
            _ => return Err(InfraError::Unimplemented),
        };

        let market = cli_perp_to_arcus_market(single_ws_inst("Arcus", insts)?)?;

        Ok(ws_subscribe_msg_arcus(stream, Some(&market), n_levels))
    }

    fn _get_public_connect_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        match channel {
            WsChannel::Lob(_) | WsChannel::Trades(_) | WsChannel::Other(_) => Ok(ARCUS_WS.into()),
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
        let cli = ArcusCli::default();
        let nvda = vec!["NVDA_USD_PERP".to_string()];

        assert_eq!(
            msg(cli._get_public_sub_msg(&WsChannel::Lob(None), Some(&nvda))),
            json!({"type":"subscribe","channel":"l2OrderbookUpdates","id":"NVDA-USD"})
        );
        assert_eq!(
            msg(cli._get_public_sub_msg(
                &WsChannel::Lob(Some(LobParam::Snapshot {
                    depth: Some(5),
                    frequency: None
                })),
                Some(&nvda)
            )),
            json!({"type":"subscribe","channel":"l2Orderbook","id":"NVDA-USD","nLevels":5})
        );
        assert_eq!(
            msg(cli._get_public_sub_msg(
                &WsChannel::Lob(Some(LobParam::Bbo { frequency: None })),
                Some(&nvda)
            )),
            json!({"type":"subscribe","channel":"bbo","id":"NVDA-USD"})
        );
        assert_eq!(
            msg(cli._get_public_sub_msg(&WsChannel::Trades(None), Some(&nvda))),
            json!({"type":"subscribe","channel":"trades","id":"NVDA-USD"})
        );
        assert_eq!(
            msg(cli._get_public_sub_msg(&WsChannel::Other("markets".into()), None)),
            json!({"type":"subscribe","channel":"markets"})
        );
    }

    #[test]
    fn public_sub_msgs_reject_bad_instruments_and_channels() {
        let cli = ArcusCli::default();

        assert!(
            cli._get_public_sub_msg(&WsChannel::Lob(None), None)
                .is_err()
        );
        assert!(
            cli._get_public_sub_msg(&WsChannel::Trades(None), Some(&["NVDA-USD".to_string()]))
                .is_err()
        );
        assert!(matches!(
            cli._get_public_sub_msg(&WsChannel::AccountOrders, None),
            Err(InfraError::Unimplemented)
        ));
        assert!(matches!(
            cli._get_public_connect_msg(&WsChannel::AccountPositions),
            Err(InfraError::Unimplemented)
        ));
        assert_eq!(
            cli._get_public_connect_msg(&WsChannel::Lob(None)).unwrap(),
            ARCUS_WS
        );
    }
}
