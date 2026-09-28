use futures_util::future::try_join_all;
use reqwest::Client;
use serde_json::json;
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
    config_assets::*,
    grvt_rest_msg::RestResGrvt,
    schemas::rest::{
        instruments::InstrumentGrvt, mini_ticker::MiniTickerGrvt, orderbook::RestOrderBookGrvt,
    },
};

#[derive(Clone, Debug)]
pub struct GrvtCli {
    pub client: Arc<Client>,
}

impl Default for GrvtCli {
    fn default() -> Self {
        Self::new(Arc::new(Client::new()))
    }
}

impl LobPublicRest for GrvtCli {
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

impl LobWebsocket for GrvtCli {
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

impl GrvtCli {
    pub fn new(shared_client: Arc<Client>) -> Self {
        Self {
            client: shared_client,
        }
    }

    /// Every perpetual, delisted ones included, with the fields [`InstrumentInfo`] drops.
    pub async fn get_all_instruments(&self) -> InfraResult<Vec<InstrumentGrvt>> {
        let url = [GRVT_BASE_URL, GRVT_ALL_INSTRUMENTS].concat();

        let response = self
            .client
            .post(url)
            .json(&json!({ "is_active": false }))
            .send()
            .await?;
        let res: RestResGrvt<InstrumentGrvt> =
            parse_json_response("GRVT all_instruments", response).await?;

        res.into_vec()
    }

    /// One instrument's mark, index and last price with its top of book.
    pub async fn get_mini_ticker(&self, inst: &str) -> InfraResult<MiniTickerGrvt> {
        let url = [GRVT_BASE_URL, GRVT_MINI_TICKER].concat();

        let response = self
            .client
            .post(url)
            .json(&json!({ "instrument": cli_perp_to_grvt_inst(inst)? }))
            .send()
            .await?;
        let res: RestResGrvt<MiniTickerGrvt> =
            parse_json_response("GRVT mini_ticker", response).await?;

        res.into_one()
    }

    /// GRVT quotes one instrument per request; without `insts` every live perpetual is asked.
    async fn _get_mini_tickers(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<MiniTickerGrvt>> {
        if inst_type
            .as_ref()
            .is_some_and(|t| *t != InstrumentType::Perpetual)
        {
            return Ok(Vec::new());
        }

        let insts = match insts {
            Some(list) => list.to_vec(),
            None => {
                self._get_live_instruments(InstrumentType::Perpetual)
                    .await?
            },
        };

        try_join_all(insts.iter().map(|inst| self.get_mini_ticker(inst))).await
    }

    async fn _get_tickers(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<TickerData>> {
        let data = self
            ._get_mini_tickers(insts, inst_type)
            .await?
            .into_iter()
            .filter_map(MiniTickerGrvt::into_ticker_data)
            .collect();

        Ok(data)
    }

    async fn _get_mark_prices(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<MarkPriceData>> {
        let data = self
            ._get_mini_tickers(insts, inst_type)
            .await?
            .into_iter()
            .filter_map(MiniTickerGrvt::into_mark_price_data)
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
                "GRVT orderbook supports perpetual instruments only, got {:?}",
                inst_type
            )));
        }

        let url = [GRVT_BASE_URL, GRVT_BOOK].concat();
        let body = json!({
            "instrument": cli_perp_to_grvt_inst(inst)?,
            "depth": grvt_orderbook_depth(depth)?,
        });

        let response = self.client.post(url).json(&body).send().await?;
        let res: RestResGrvt<RestOrderBookGrvt> =
            parse_json_response("GRVT orderbook", response).await?;

        res.into_one().map(|entry| entry.into_orderbook_data(inst))
    }

    async fn _get_instrument_info(
        &self,
        inst_type: InstrumentType,
    ) -> InfraResult<Vec<InstrumentInfo>> {
        let data = self
            .get_all_instruments()
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
        let (stream, secondary) = match channel {
            WsChannel::Lob(lob_param) => grvt_lob_stream(lob_param)?,
            WsChannel::Trades(trades_param) => grvt_trades_stream(trades_param)?,
            WsChannel::Other(feed) => {
                let (stream, secondary) = feed.split_once('@').ok_or_else(|| {
                    InfraError::ApiCliError(format!(
                        "GRVT raw feeds are <stream>@<selector suffix>, got {feed}"
                    ))
                })?;
                return Ok(ws_subscribe_msg_grvt(
                    stream,
                    &grvt_selectors(insts, secondary)?,
                ));
            },
            _ => return Err(InfraError::Unimplemented),
        };

        Ok(ws_subscribe_msg_grvt(
            stream,
            &grvt_selectors(insts, &secondary)?,
        ))
    }

    fn _get_public_connect_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        match channel {
            WsChannel::Lob(_) | WsChannel::Trades(_) | WsChannel::Other(_) => Ok(GRVT_WS.into()),
            _ => Err(InfraError::Unimplemented),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    fn params(res: InfraResult<String>) -> Value {
        serde_json::from_str::<Value>(&res.unwrap()).unwrap()["params"].clone()
    }

    #[test]
    fn public_sub_msgs_follow_the_channel() {
        let cli = GrvtCli::default();
        let btc = vec!["BTC_USDT_PERP".to_string()];

        assert_eq!(
            params(cli._get_public_sub_msg(&WsChannel::Lob(None), Some(&btc))),
            json!({"stream":"v1.book.d","selectors":["BTC_USDT_Perp@50"]})
        );
        assert_eq!(
            params(cli._get_public_sub_msg(
                &WsChannel::Lob(Some(LobParam::Snapshot {
                    depth: Some(50),
                    frequency: Some(LobFrequency::Ms1000)
                })),
                Some(&btc)
            )),
            json!({"stream":"v1.book.s","selectors":["BTC_USDT_Perp@1000-50"]})
        );
        assert_eq!(
            params(cli._get_public_sub_msg(
                &WsChannel::Lob(Some(LobParam::Bbo { frequency: None })),
                Some(&btc)
            )),
            json!({"stream":"v1.mini.s","selectors":["BTC_USDT_Perp@200"]})
        );
        assert_eq!(
            params(cli._get_public_sub_msg(&WsChannel::Trades(None), Some(&btc))),
            json!({"stream":"v1.trade","selectors":["BTC_USDT_Perp@50"]})
        );
        assert_eq!(
            params(
                cli._get_public_sub_msg(&WsChannel::Other("v1.ticker.d@500".into()), Some(&btc))
            ),
            json!({"stream":"v1.ticker.d","selectors":["BTC_USDT_Perp@500"]})
        );
    }

    #[test]
    fn one_message_subscribes_every_instrument() {
        let cli = GrvtCli::default();
        let insts = vec!["BTC_USDT_PERP".to_string(), "NVDA_USDT_PERP".to_string()];

        assert_eq!(
            params(cli._get_public_sub_msg(&WsChannel::Trades(None), Some(&insts)))["selectors"],
            json!(["BTC_USDT_Perp@50", "NVDA_USDT_Perp@50"])
        );
    }

    #[test]
    fn public_sub_msgs_reject_bad_instruments_and_channels() {
        let cli = GrvtCli::default();
        let btc = vec!["BTC_USDT_PERP".to_string()];

        assert!(
            cli._get_public_sub_msg(&WsChannel::Lob(None), None)
                .is_err()
        );
        assert!(
            cli._get_public_sub_msg(&WsChannel::Lob(None), Some(&["BTC_USDT_Perp".to_string()]))
                .is_err()
        );
        assert!(
            cli._get_public_sub_msg(&WsChannel::Other("v1.ticker.d".into()), Some(&btc))
                .is_err()
        );
        assert!(
            cli._get_public_sub_msg(&WsChannel::Trades(Some(TradesParam::AggTrades)), Some(&btc))
                .is_err()
        );
        assert!(matches!(
            cli._get_public_sub_msg(&WsChannel::Candles(None), Some(&btc)),
            Err(InfraError::Unimplemented)
        ));
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
            GRVT_WS
        );
    }

    #[tokio::test]
    async fn rest_requests_reject_bad_arguments_before_sending() {
        let cli = GrvtCli::default();

        for (inst, inst_type, depth) in [
            ("BTC_USDT_PERP", InstrumentType::Spot, 10),
            ("BTC_USDT_PERP", InstrumentType::Perpetual, 5),
            ("BTC_USDT_Perp", InstrumentType::Perpetual, 10),
        ] {
            assert!(matches!(
                cli.get_orderbook(inst, inst_type, depth).await,
                Err(InfraError::ApiCliError(_))
            ));
        }
        assert!(matches!(
            cli.get_mini_ticker("BTCUSDT").await,
            Err(InfraError::ApiCliError(_))
        ));
        assert!(
            cli.get_tickers(None, Some(InstrumentType::Spot))
                .await
                .unwrap()
                .is_empty()
        );
    }
}
