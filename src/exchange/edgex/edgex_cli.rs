use futures_util::{StreamExt, TryStreamExt, stream};
use reqwest::Client;
use std::sync::Arc;

use extrema_infra::{
    arch::market_assets::{
        api_data::{price_data::*, utils_data::*},
        api_general::parse_json_response,
    },
    prelude::*,
};

use crate::exchange::api_general::{exactly_one, single_ws_inst};

use super::{
    api_utils::*,
    config_assets::*,
    edgex_rest_msg::RestResEdgex,
    schemas::rest::{depth::RestDepthEdgex, meta_data::RestMetaDataEdgex, ticker::RestTickerEdgex},
};

#[derive(Clone, Debug)]
pub struct EdgexCli {
    pub client: Arc<Client>,
}

impl Default for EdgexCli {
    fn default() -> Self {
        Self::new(Arc::new(Client::new()))
    }
}

impl LobPublicRest for EdgexCli {
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

impl LobWebsocket for EdgexCli {
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

impl EdgexCli {
    pub fn new(shared_client: Arc<Client>) -> Self {
        Self {
            client: shared_client,
        }
    }

    /// Coins and contracts, with the stock flag, fees and risk tiers that [`InstrumentInfo`] drops.
    pub async fn get_meta_data(&self) -> InfraResult<RestMetaDataEdgex> {
        let url = [EDGEX_BASE_URL, EDGEX_META_DATA].concat();

        let response = self.client.get(url).send().await?;
        let res: RestResEdgex<RestMetaDataEdgex> =
            parse_json_response("edgeX meta_data", response).await?;

        res.into_one()
    }

    /// edgeX quotes one contract per request, so `None` fans out over every contract.
    async fn _get_contract_tickers(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<(u64, RestTickerEdgex)>> {
        if inst_type
            .as_ref()
            .is_some_and(|t| *t != InstrumentType::Perpetual)
        {
            return Ok(Vec::new());
        }

        let contract_ids: Vec<u64> = match insts {
            Some(list) => list
                .iter()
                .map(|inst| cli_to_edgex_contract_id(inst))
                .collect::<InfraResult<_>>()?,
            None => self
                .get_meta_data()
                .await?
                .contractList
                .into_iter()
                .map(|contract| contract.contractId)
                .collect(),
        };

        let tickers: Vec<Vec<(u64, RestTickerEdgex)>> = stream::iter(contract_ids)
            .map(|contract_id| self._get_contract_ticker(contract_id))
            .buffered(EDGEX_TICKER_CONCURRENCY)
            .try_collect()
            .await?;

        Ok(tickers.into_iter().flatten().collect())
    }

    /// The contract's ticker stamped with the reply time; an unknown contract has none.
    async fn _get_contract_ticker(
        &self,
        contract_id: u64,
    ) -> InfraResult<Vec<(u64, RestTickerEdgex)>> {
        let url = format!(
            "{}{}?contractId={}",
            EDGEX_BASE_URL, EDGEX_TICKER, contract_id
        );

        let response = self.client.get(url).send().await?;
        let res: RestResEdgex<Vec<RestTickerEdgex>> =
            parse_json_response("edgeX ticker", response).await?;
        let timestamp = res.response_time();

        let data = res
            .into_one()?
            .into_iter()
            .map(|ticker| (timestamp, ticker))
            .collect();

        Ok(data)
    }

    async fn _get_tickers(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<TickerData>> {
        let data = self
            ._get_contract_tickers(insts, inst_type)
            .await?
            .into_iter()
            .filter_map(|(timestamp, ticker)| ticker.into_ticker_data(timestamp))
            .collect();

        Ok(data)
    }

    async fn _get_mark_prices(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<MarkPriceData>> {
        let data = self
            ._get_contract_tickers(insts, inst_type)
            .await?
            .into_iter()
            .filter_map(|(timestamp, ticker)| ticker.into_mark_price_data(timestamp))
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
                "edgeX orderbook supports perpetual instruments only, got {:?}",
                inst_type
            )));
        }

        let url = format!(
            "{}{}?contractId={}&level={}",
            EDGEX_BASE_URL,
            EDGEX_DEPTH,
            cli_to_edgex_contract_id(inst)?,
            edgex_orderbook_level(depth)?
        );

        let response = self.client.get(url).send().await?;
        let res: RestResEdgex<Vec<RestDepthEdgex>> =
            parse_json_response("edgeX orderbook", response).await?;
        let timestamp = res.response_time();

        let books = res.into_one()?;
        if books.is_empty() {
            return Err(InfraError::ApiCliError(format!(
                "edgeX has no contract {inst}"
            )));
        }

        exactly_one(books).map(|book| book.into_orderbook_data(timestamp))
    }

    async fn _get_instrument_info(
        &self,
        inst_type: InstrumentType,
    ) -> InfraResult<Vec<InstrumentInfo>> {
        let data = self
            .get_meta_data()
            .await?
            .contractList
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
        let contract_id = || cli_to_edgex_contract_id(single_ws_inst("edgeX", insts)?);

        let channel = match channel {
            WsChannel::Lob(Some(LobParam::Bbo { frequency })) => {
                if insts.is_some() {
                    return Err(InfraError::ApiCliError(
                        "edgeX publishes BBO for every market in one stream; subscribe without instruments"
                            .into(),
                    ));
                }
                edgex_bbo_channel(frequency)?.to_string()
            },
            WsChannel::Lob(lob_param) => {
                let level = edgex_depth_level(lob_param)?;
                format!("depth.{}.{}", contract_id()?, level)
            },
            WsChannel::Trades(trades_param) => {
                format!("{}.{}", edgex_trades_channel(trades_param)?, contract_id()?)
            },
            WsChannel::Other(stream) => match insts {
                Some(_) => format!("{}.{}", stream, contract_id()?),
                None => stream.clone(),
            },
            _ => return Err(InfraError::Unimplemented),
        };

        Ok(ws_subscribe_msg_edgex(&channel))
    }

    fn _get_public_connect_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        match channel {
            WsChannel::Lob(_) | WsChannel::Trades(_) | WsChannel::Other(_) => Ok(EDGEX_WS.into()),
            _ => Err(InfraError::Unimplemented),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn channel_of(msg: InfraResult<String>) -> String {
        let msg: Value = serde_json::from_str(&msg.unwrap()).unwrap();
        assert_eq!(msg["type"], "subscribe");
        msg["channel"].as_str().unwrap().to_string()
    }

    fn incremental(depth: Option<u16>) -> WsChannel {
        WsChannel::Lob(Some(LobParam::Incremental {
            depth,
            frequency: None,
        }))
    }

    #[test]
    fn public_sub_msgs_follow_the_channel() {
        let cli = EdgexCli::default();
        let nvda = vec!["@30000020".to_string()];

        assert_eq!(
            channel_of(cli._get_public_sub_msg(&WsChannel::Lob(None), Some(&nvda))),
            "depth.30000020.200"
        );
        assert_eq!(
            channel_of(cli._get_public_sub_msg(&incremental(Some(15)), Some(&nvda))),
            "depth.30000020.15"
        );
        assert_eq!(
            channel_of(cli._get_public_sub_msg(&incremental(None), Some(&nvda))),
            "depth.30000020.200"
        );
        assert_eq!(
            channel_of(cli._get_public_sub_msg(&WsChannel::Trades(None), Some(&nvda))),
            "trades.30000020"
        );
        assert_eq!(
            channel_of(cli._get_public_sub_msg(
                &WsChannel::Lob(Some(LobParam::Bbo { frequency: None })),
                None
            )),
            "bookTicker.all.1s"
        );
        assert_eq!(
            channel_of(cli._get_public_sub_msg(&WsChannel::Other("ticker".into()), Some(&nvda))),
            "ticker.30000020"
        );
        assert_eq!(
            channel_of(cli._get_public_sub_msg(&WsChannel::Other("ticker.all.1s".into()), None)),
            "ticker.all.1s"
        );
        assert_eq!(
            serde_json::from_str::<Value>(
                &cli._get_public_sub_msg(&WsChannel::Trades(None), Some(&nvda))
                    .unwrap()
            )
            .unwrap(),
            json!({"type":"subscribe","channel":"trades.30000020"})
        );
    }

    #[test]
    fn public_sub_msgs_reject_bad_instruments_and_channels() {
        let cli = EdgexCli::default();
        let nvda = vec!["@30000020".to_string()];
        let bbo = WsChannel::Lob(Some(LobParam::Bbo { frequency: None }));

        assert!(
            cli._get_public_sub_msg(&WsChannel::Lob(None), None)
                .is_err()
        );
        assert!(
            cli._get_public_sub_msg(&WsChannel::Trades(None), Some(&["NVDAUSDC".to_string()]))
                .is_err()
        );
        assert!(cli._get_public_sub_msg(&bbo, Some(&nvda)).is_err());
        assert!(cli._get_public_sub_msg(&bbo, Some(&[])).is_err());
        assert!(
            cli._get_public_sub_msg(&incremental(Some(20)), Some(&nvda))
                .is_err()
        );
        assert!(
            cli._get_public_sub_msg(
                &WsChannel::Trades(Some(TradesParam::AggTrades)),
                Some(&nvda)
            )
            .is_err()
        );
        assert!(matches!(
            cli._get_public_sub_msg(&WsChannel::AccountOrders, None),
            Err(InfraError::Unimplemented)
        ));
        assert!(matches!(
            cli._get_public_sub_msg(&WsChannel::Candles(None), Some(&nvda)),
            Err(InfraError::Unimplemented)
        ));
        assert!(matches!(
            cli._get_public_connect_msg(&WsChannel::AccountPositions),
            Err(InfraError::Unimplemented)
        ));
        for channel in [WsChannel::Lob(None), bbo, WsChannel::Trades(None)] {
            assert_eq!(cli._get_public_connect_msg(&channel).unwrap(), EDGEX_WS);
        }
    }

    #[tokio::test]
    async fn rest_rejects_bad_arguments_before_any_request() {
        let cli = EdgexCli::default();

        assert!(
            cli.get_orderbook("@30000020", InstrumentType::Spot, 15)
                .await
                .is_err()
        );
        assert!(
            cli.get_orderbook("@30000020", InstrumentType::Perpetual, 5)
                .await
                .is_err()
        );
        assert!(
            cli.get_orderbook("NVDAUSDC", InstrumentType::Perpetual, 15)
                .await
                .is_err()
        );
        assert!(
            cli.get_tickers(Some(&["NVDAUSDC".to_string()]), None)
                .await
                .is_err()
        );
        assert!(
            cli.get_mark_prices(None, Some(InstrumentType::Spot))
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            cli.get_tickers(Some(&[]), Some(InstrumentType::Perpetual))
                .await
                .unwrap()
                .is_empty()
        );
    }
}
