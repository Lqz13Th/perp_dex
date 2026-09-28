use reqwest::{Client, header::USER_AGENT};
use std::sync::Arc;

use extrema_infra::{
    arch::market_assets::{
        api_data::{price_data::*, utils_data::*},
        api_general::{get_micros_timestamp, parse_json_response},
    },
    prelude::*,
};

use super::{
    api_utils::*,
    config_assets::*,
    extended_rest_msg::RestResExtended,
    schemas::rest::{markets::MarketExtended, orderbook::RestOrderBookExtended},
};

const EXTENDED_URL_IS_THE_SUBSCRIPTION: &str = "Extended streams one market per URL and reads nothing from the client; \
     connect with ExtendedCli::get_public_stream_target(channel, inst) and send no subscribe message";

#[derive(Clone, Debug)]
pub struct ExtendedCli {
    pub client: Arc<Client>,
}

impl Default for ExtendedCli {
    fn default() -> Self {
        Self::new(Arc::new(Client::new()))
    }
}

impl LobPublicRest for ExtendedCli {
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

impl LobWebsocket for ExtendedCli {
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

    async fn get_public_connect_target(&self, channel: &WsChannel) -> InfraResult<WsConnectTarget> {
        self._get_public_connect_msg(channel)
            .map(WsConnectTarget::new)
    }
}

impl ExtendedCli {
    pub fn new(shared_client: Arc<Client>) -> Self {
        Self {
            client: shared_client,
        }
    }

    /// Markets with the category, trading hours and stats that [`InstrumentInfo`] drops.
    ///
    /// `insts` are perpetuals, filtered on the venue, which fails the whole request on an unknown market.
    pub async fn get_markets(&self, insts: Option<&[String]>) -> InfraResult<Vec<MarketExtended>> {
        let mut url = [EXTENDED_BASE_URL, EXTENDED_MARKETS].concat();

        if let Some(insts) = insts {
            let markets = insts
                .iter()
                .map(|inst| {
                    cli_perp_to_extended_market(inst).map(|market| format!("market={market}"))
                })
                .collect::<InfraResult<Vec<_>>>()?;
            url.push_str(&format!("?{}", markets.join("&")));
        }

        let response = self
            .client
            .get(url)
            .header(USER_AGENT, EXTENDED_USER_AGENT)
            .send()
            .await?;
        let res: RestResExtended<MarketExtended> =
            parse_json_response("Extended markets", response).await?;

        res.into_vec()
    }

    /// The websocket target for `channel`, which is the whole subscription: Extended
    /// streams one market per URL and reads nothing from the client, so connect with
    /// `TaskCommand::WsConnectWithTarget` and send no subscribe message. The target
    /// carries the `User-Agent` without which the upgrade is refused with 403.
    ///
    /// `Lob` and `Trades` need an instrument. `Other(stream)` is a raw stream path
    /// such as `prices/mark`; without an instrument it carries every market.
    pub fn get_public_stream_target(
        &self,
        channel: &WsChannel,
        inst: Option<&str>,
    ) -> InfraResult<WsConnectTarget> {
        let url = self._get_public_stream_url(channel, inst)?;

        Ok(WsConnectTarget::new(url).with_header(USER_AGENT.as_str(), EXTENDED_USER_AGENT))
    }

    fn _get_public_stream_url(
        &self,
        channel: &WsChannel,
        inst: Option<&str>,
    ) -> InfraResult<String> {
        let (stream, depth) = match channel {
            WsChannel::Lob(lob_param) => extended_lob_stream(lob_param)?,
            WsChannel::Trades(trades_param) => (extended_trades_stream(trades_param)?, None),
            WsChannel::Other(stream) => {
                let market = inst.map(cli_perp_to_extended_market).transpose()?;
                return Ok(ws_stream_url_extended(stream, market.as_deref(), None));
            },
            _ => return Err(InfraError::Unimplemented),
        };

        let Some(inst) = inst else {
            return Err(InfraError::ApiCliError(format!(
                "Extended {stream} streams need an instrument"
            )));
        };

        Ok(ws_stream_url_extended(
            stream,
            Some(&cli_perp_to_extended_market(inst)?),
            depth,
        ))
    }

    async fn _get_tickers(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<TickerData>> {
        let timestamp = get_micros_timestamp();
        let data = self
            .get_markets(None)
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
            .get_markets(None)
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
                "Extended orderbook supports perpetual instruments only, got {:?}",
                inst_type
            )));
        }

        let url = format!(
            "{}{}/{}{}",
            EXTENDED_BASE_URL,
            EXTENDED_MARKETS,
            cli_perp_to_extended_market(inst)?,
            EXTENDED_ORDER_BOOK
        );

        let response = self
            .client
            .get(url)
            .header(USER_AGENT, EXTENDED_USER_AGENT)
            .send()
            .await?;
        let res: RestResExtended<RestOrderBookExtended> =
            parse_json_response("Extended orderbook", response).await?;

        res.into_one()
            .map(|entry| entry.into_orderbook_data(inst, depth))
    }

    async fn _get_instrument_info(
        &self,
        inst_type: InstrumentType,
    ) -> InfraResult<Vec<InstrumentInfo>> {
        let data = self
            .get_markets(None)
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
        _insts: Option<&[String]>,
    ) -> InfraResult<String> {
        match channel {
            WsChannel::Lob(_) | WsChannel::Trades(_) | WsChannel::Other(_) => Err(
                InfraError::ApiCliError(EXTENDED_URL_IS_THE_SUBSCRIPTION.into()),
            ),
            _ => Err(InfraError::Unimplemented),
        }
    }

    fn _get_public_connect_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        match channel {
            WsChannel::Lob(_) | WsChannel::Trades(_) | WsChannel::Other(_) => Err(
                InfraError::ApiCliError(EXTENDED_URL_IS_THE_SUBSCRIPTION.into()),
            ),
            _ => Err(InfraError::Unimplemented),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NVDA: &str = "NVDA_24_5_USD_PERP";

    fn url(channel: WsChannel, inst: Option<&str>) -> String {
        ExtendedCli::default()
            ._get_public_stream_url(&channel, inst)
            .unwrap()
    }

    #[test]
    fn stream_urls_follow_the_channel() {
        assert_eq!(
            url(WsChannel::Lob(None), Some(NVDA)),
            format!("{EXTENDED_WS}/orderbooks/NVDA_24_5-USD")
        );
        assert_eq!(
            url(
                WsChannel::Lob(Some(LobParam::Incremental {
                    depth: None,
                    frequency: Some(LobFrequency::Ms100)
                })),
                Some("BTC_USD_PERP")
            ),
            format!("{EXTENDED_WS}/orderbooks/BTC-USD")
        );
        assert_eq!(
            url(
                WsChannel::Lob(Some(LobParam::Bbo { frequency: None })),
                Some(NVDA)
            ),
            format!("{EXTENDED_WS}/orderbooks/NVDA_24_5-USD?depth=1")
        );
        assert_eq!(
            url(WsChannel::Trades(None), Some(NVDA)),
            format!("{EXTENDED_WS}/publicTrades/NVDA_24_5-USD")
        );
        assert_eq!(
            url(WsChannel::Other("prices/mark".into()), Some("BTC_USD_PERP")),
            format!("{EXTENDED_WS}/prices/mark/BTC-USD")
        );
        assert_eq!(
            url(WsChannel::Other("publicTrades".into()), None),
            format!("{EXTENDED_WS}/publicTrades")
        );
    }

    #[test]
    fn stream_target_carries_a_user_agent() {
        let target = ExtendedCli::default()
            .get_public_stream_target(&WsChannel::Trades(None), Some(NVDA))
            .unwrap();

        assert_eq!(
            target.url,
            format!("{EXTENDED_WS}/publicTrades/NVDA_24_5-USD")
        );
        assert_eq!(
            target.headers,
            vec![("user-agent".to_string(), EXTENDED_USER_AGENT.to_string())]
        );
    }

    #[test]
    fn stream_urls_reject_bad_instruments_and_channels() {
        let cli = ExtendedCli::default();

        assert!(matches!(
            cli._get_public_stream_url(&WsChannel::Lob(None), None),
            Err(InfraError::ApiCliError(_))
        ));
        assert!(
            cli._get_public_stream_url(&WsChannel::Trades(None), Some("NVDA-USD"))
                .is_err()
        );
        assert!(
            cli._get_public_stream_url(
                &WsChannel::Trades(Some(TradesParam::AggTrades)),
                Some(NVDA)
            )
            .is_err()
        );
        assert!(
            cli._get_public_stream_url(
                &WsChannel::Lob(Some(LobParam::Snapshot {
                    depth: Some(5),
                    frequency: None
                })),
                Some(NVDA)
            )
            .is_err()
        );
        assert!(matches!(
            cli._get_public_stream_url(&WsChannel::AccountOrders, Some(NVDA)),
            Err(InfraError::Unimplemented)
        ));
        assert!(matches!(
            cli._get_public_stream_url(&WsChannel::Candles(None), Some(NVDA)),
            Err(InfraError::Unimplemented)
        ));
    }

    #[test]
    fn trait_ws_messages_point_to_the_stream_url() {
        let cli = ExtendedCli::default();
        let nvda = vec![NVDA.to_string()];

        for channel in [
            WsChannel::Lob(None),
            WsChannel::Trades(None),
            WsChannel::Other("prices/mark".into()),
        ] {
            let Err(InfraError::ApiCliError(msg)) = cli._get_public_connect_msg(&channel) else {
                panic!("{channel:?} has a connect message");
            };
            assert!(msg.contains("get_public_stream_target"), "{msg}");
            assert!(matches!(
                cli._get_public_sub_msg(&channel, Some(&nvda)),
                Err(InfraError::ApiCliError(_))
            ));
        }
        assert!(matches!(
            cli._get_public_connect_msg(&WsChannel::AccountOrders),
            Err(InfraError::Unimplemented)
        ));
        assert!(matches!(
            cli._get_public_sub_msg(&WsChannel::AccountPositions, None),
            Err(InfraError::Unimplemented)
        ));
    }
}
