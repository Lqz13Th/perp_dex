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
    config_assets::*,
    lighter_rest_msg::RestResLighter,
    schemas::rest::{
        order_book_details::RestOrderBookDetailsLighter,
        order_book_orders::RestOrderBookOrdersLighter,
    },
};

#[derive(Clone, Debug)]
pub struct LighterCli {
    pub client: Arc<Client>,
    pub venue: LighterVenue,
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

impl LobWebsocket for LighterCli {
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

impl LighterCli {
    pub fn new(shared_client: Arc<Client>) -> Self {
        Self {
            client: shared_client,
            venue: LighterVenue::Mainnet,
        }
    }

    /// Market ids differ between venues, so one client serves one venue.
    pub fn set_venue(&mut self, venue: LighterVenue) {
        self.venue = venue;
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

    fn _get_public_connect_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        match channel {
            WsChannel::Lob(_) | WsChannel::Trades(_) | WsChannel::Other(_) => {
                Ok(self.venue.ws_url().into())
            },
            _ => Err(InfraError::Unimplemented),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

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
