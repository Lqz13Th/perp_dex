use extrema_infra::{
    arch::market_assets::{
        api_data::{price_data::*, utils_data::*},
        exchange::prelude::HyperliquidCli,
    },
    prelude::*,
};

#[cfg(feature = "apex")]
use super::apex::{apex_cli::ApexCli, config_assets::APEX};
#[cfg(feature = "arcus")]
use super::arcus::{arcus_cli::ArcusCli, config_assets::ARCUS};
#[cfg(feature = "aster")]
use super::aster::{aster_cli::AsterCli, config_assets::ASTER};
#[cfg(feature = "edgex")]
use super::edgex::{config_assets::EDGEX, edgex_cli::EdgexCli};
#[cfg(feature = "extended")]
use super::extended::{config_assets::EXTENDED, extended_cli::ExtendedCli};
#[cfg(feature = "grvt")]
use super::grvt::{config_assets::GRVT, grvt_cli::GrvtCli};
#[cfg(feature = "lighter")]
use super::lighter::lighter_cli::LighterCli;
#[cfg(feature = "nado")]
use super::nado::{config_assets::NADO, nado_cli::NadoCli};
#[cfg(feature = "pacifica")]
use super::pacifica::{config_assets::PACIFICA, pacifica_cli::PacificaCli};

/// Unified dispatcher for the stock-perp venues, analogous to infra's `LobClients`.
///
/// Hyperliquid builder DEXes use infra's `HyperliquidCli` with `set_perp_dex`.
#[derive(Clone, Debug)]
pub enum PerpDexClients {
    Hyperliquid(HyperliquidCli),
    #[cfg(feature = "lighter")]
    Lighter(LighterCli),
    #[cfg(feature = "aster")]
    Aster(AsterCli),
    #[cfg(feature = "arcus")]
    Arcus(ArcusCli),
    #[cfg(feature = "grvt")]
    Grvt(GrvtCli),
    #[cfg(feature = "pacifica")]
    Pacifica(PacificaCli),
    #[cfg(feature = "edgex")]
    Edgex(EdgexCli),
    #[cfg(feature = "nado")]
    Nado(NadoCli),
    #[cfg(feature = "apex")]
    Apex(ApexCli),
    /// Streams one market per URL; connect with `ExtendedCli::get_public_stream_target`.
    #[cfg(feature = "extended")]
    Extended(ExtendedCli),
}

impl Default for PerpDexClients {
    fn default() -> Self {
        PerpDexClients::Hyperliquid(HyperliquidCli::default())
    }
}

macro_rules! dispatch {
    ($self:ident, $c:ident => $call:expr) => {
        match $self {
            PerpDexClients::Hyperliquid($c) => $call,
            #[cfg(feature = "lighter")]
            PerpDexClients::Lighter($c) => $call,
            #[cfg(feature = "aster")]
            PerpDexClients::Aster($c) => $call,
            #[cfg(feature = "arcus")]
            PerpDexClients::Arcus($c) => $call,
            #[cfg(feature = "grvt")]
            PerpDexClients::Grvt($c) => $call,
            #[cfg(feature = "pacifica")]
            PerpDexClients::Pacifica($c) => $call,
            #[cfg(feature = "edgex")]
            PerpDexClients::Edgex($c) => $call,
            #[cfg(feature = "nado")]
            PerpDexClients::Nado($c) => $call,
            #[cfg(feature = "apex")]
            PerpDexClients::Apex($c) => $call,
            #[cfg(feature = "extended")]
            PerpDexClients::Extended($c) => $call,
        }
    };
}

impl PerpDexClients {
    pub fn market(&self) -> Market {
        match self {
            PerpDexClients::Hyperliquid(_) => Market::HyperLiquid,
            #[cfg(feature = "lighter")]
            PerpDexClients::Lighter(c) => c.venue.market(),
            #[cfg(feature = "aster")]
            PerpDexClients::Aster(_) => ASTER,
            #[cfg(feature = "arcus")]
            PerpDexClients::Arcus(_) => ARCUS,
            #[cfg(feature = "grvt")]
            PerpDexClients::Grvt(_) => GRVT,
            #[cfg(feature = "pacifica")]
            PerpDexClients::Pacifica(_) => PACIFICA,
            #[cfg(feature = "edgex")]
            PerpDexClients::Edgex(_) => EDGEX,
            #[cfg(feature = "nado")]
            PerpDexClients::Nado(_) => NADO,
            #[cfg(feature = "apex")]
            PerpDexClients::Apex(_) => APEX,
            #[cfg(feature = "extended")]
            PerpDexClients::Extended(_) => EXTENDED,
        }
    }
}

impl LobPublicRest for PerpDexClients {
    async fn get_tickers(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<TickerData>> {
        dispatch!(self, c => c.get_tickers(insts, inst_type).await)
    }

    async fn get_mark_prices(
        &self,
        insts: Option<&[String]>,
        inst_type: Option<InstrumentType>,
    ) -> InfraResult<Vec<MarkPriceData>> {
        dispatch!(self, c => c.get_mark_prices(insts, inst_type).await)
    }

    async fn get_orderbook(
        &self,
        inst: &str,
        inst_type: InstrumentType,
        depth: usize,
    ) -> InfraResult<OrderBookData> {
        dispatch!(self, c => c.get_orderbook(inst, inst_type, depth).await)
    }

    async fn get_candles(
        &self,
        inst: &str,
        inst_type: InstrumentType,
        interval: CandleParam,
        limit: Option<u32>,
        start_time_us: Option<u64>,
        end_time_us: Option<u64>,
    ) -> InfraResult<Vec<CandleData>> {
        dispatch!(self, c => {
            c.get_candles(inst, inst_type, interval, limit, start_time_us, end_time_us)
                .await
        })
    }

    async fn get_instrument_info(
        &self,
        inst_type: InstrumentType,
    ) -> InfraResult<Vec<InstrumentInfo>> {
        dispatch!(self, c => c.get_instrument_info(inst_type).await)
    }

    async fn get_live_instruments(&self, inst_type: InstrumentType) -> InfraResult<Vec<String>> {
        dispatch!(self, c => c.get_live_instruments(inst_type).await)
    }
}

impl LobWebsocket for PerpDexClients {
    async fn get_public_sub_msg(
        &self,
        channel: &WsChannel,
        insts: Option<&[String]>,
    ) -> InfraResult<String> {
        dispatch!(self, c => c.get_public_sub_msg(channel, insts).await)
    }

    async fn get_private_sub_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        dispatch!(self, c => c.get_private_sub_msg(channel).await)
    }

    async fn get_public_connect_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        dispatch!(self, c => c.get_public_connect_msg(channel).await)
    }

    async fn get_public_connect_target(&self, channel: &WsChannel) -> InfraResult<WsConnectTarget> {
        dispatch!(self, c => c.get_public_connect_target(channel).await)
    }

    async fn get_private_connect_msg(&self, channel: &WsChannel) -> InfraResult<String> {
        dispatch!(self, c => c.get_private_connect_msg(channel).await)
    }

    async fn get_private_connect_target(
        &self,
        channel: &WsChannel,
    ) -> InfraResult<WsConnectTarget> {
        dispatch!(self, c => c.get_private_connect_target(channel).await)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    struct Venue {
        client: PerpDexClients,
        market: Market,
        ws_url: &'static str,
        channel: WsChannel,
        inst: Option<&'static str>,
        sub: &'static str,
    }

    fn venues() -> Vec<Venue> {
        vec![
            Venue {
                client: PerpDexClients::Hyperliquid(HyperliquidCli::default()),
                market: Market::HyperLiquid,
                ws_url: "wss://api.hyperliquid.xyz/ws",
                channel: bbo(),
                inst: None,
                sub: "",
            },
            #[cfg(feature = "lighter")]
            Venue {
                client: PerpDexClients::Lighter(LighterCli::default()),
                market: Market::Custom(1),
                ws_url: "wss://mainnet.zklighter.elliot.ai/stream",
                channel: bbo(),
                inst: Some("@110"),
                sub: "ticker/110",
            },
            #[cfg(feature = "aster")]
            Venue {
                client: PerpDexClients::Aster(AsterCli::default()),
                market: Market::Custom(2),
                ws_url: "wss://fstream.asterdex.com/ws",
                channel: bbo(),
                inst: Some("NVDA_USDT_PERP"),
                sub: "nvdausdt@bookTicker",
            },
            #[cfg(feature = "arcus")]
            Venue {
                client: PerpDexClients::Arcus(ArcusCli::default()),
                market: Market::Custom(3),
                ws_url: "wss://api.arcus.xyz/v1/ws",
                channel: bbo(),
                inst: Some("NVDA_USD_PERP"),
                sub: "\"bbo\"",
            },
            #[cfg(feature = "grvt")]
            Venue {
                client: PerpDexClients::Grvt(GrvtCli::default()),
                market: Market::Custom(5),
                ws_url: "wss://market-data.grvt.io/ws/full",
                channel: bbo(),
                inst: Some("NVDA_USDT_PERP"),
                sub: "NVDA_USDT_Perp@200",
            },
            #[cfg(feature = "edgex")]
            Venue {
                client: PerpDexClients::Edgex(EdgexCli::default()),
                market: Market::Custom(7),
                ws_url: "wss://edgex-quote-prod-v2.edgex.exchange/api/v1/public/ws",
                channel: bbo(),
                inst: None,
                sub: "bookTicker.all.1s",
            },
            #[cfg(feature = "nado")]
            Venue {
                client: PerpDexClients::Nado(NadoCli::default()),
                market: Market::Custom(9),
                ws_url: "wss://direct-gateway.prod.nado-backend.xyz/v1/subscribe",
                channel: bbo(),
                inst: Some("@112"),
                sub: "\"best_bid_offer\"",
            },
            #[cfg(feature = "apex")]
            Venue {
                client: PerpDexClients::Apex(ApexCli::default()),
                market: Market::Custom(8),
                ws_url: "wss://quote.omni.apex.exchange/realtime_public?v=2",
                channel: WsChannel::Lob(None),
                inst: Some("NVDA_USDT_PERP"),
                sub: "orderBook200.H.NVDAUSDT",
            },
            #[cfg(feature = "pacifica")]
            Venue {
                client: PerpDexClients::Pacifica(PacificaCli::default()),
                market: Market::Custom(10),
                ws_url: "wss://ws.pacifica.fi/ws",
                channel: bbo(),
                inst: Some("NVDA_USDC_PERP"),
                sub: "\"bbo\"",
            },
        ]
    }

    fn bbo() -> WsChannel {
        WsChannel::Lob(Some(LobParam::Bbo { frequency: None }))
    }

    /// `venues` plus Extended, which connects per market instead of via the trait.
    fn all_clients() -> Vec<PerpDexClients> {
        let clients = venues().into_iter().map(|v| v.client);
        #[cfg(feature = "extended")]
        let clients = clients.chain([PerpDexClients::Extended(ExtendedCli::default())]);
        clients.collect()
    }

    #[test]
    fn each_variant_reports_its_own_market() {
        for venue in &venues() {
            assert_eq!(venue.client.market(), venue.market);
        }
        #[cfg(feature = "extended")]
        assert_eq!(
            PerpDexClients::Extended(ExtendedCli::default()).market(),
            Market::Custom(6)
        );
        let clients = all_clients();
        let ids: HashSet<Market> = clients.iter().map(PerpDexClients::market).collect();
        assert_eq!(ids.len(), clients.len(), "venue ids collide");
        assert!(matches!(
            PerpDexClients::default(),
            PerpDexClients::Hyperliquid(_)
        ));
    }

    #[cfg(feature = "lighter")]
    #[test]
    fn lighter_robinhood_is_its_own_market() {
        let mut cli = LighterCli::default();
        cli.set_venue(super::super::lighter::config_assets::LighterVenue::Robinhood);

        assert_eq!(PerpDexClients::Lighter(cli).market(), Market::Custom(4));
    }

    #[tokio::test]
    async fn public_ws_messages_dispatch_to_the_venue() {
        for venue in venues() {
            assert_eq!(
                venue
                    .client
                    .get_public_connect_msg(&venue.channel)
                    .await
                    .unwrap(),
                venue.ws_url
            );
            if venue.sub.is_empty() {
                continue;
            }
            let insts = venue.inst.map(|inst| vec![inst.to_string()]);
            let sub = venue
                .client
                .get_public_sub_msg(&venue.channel, insts.as_deref())
                .await
                .unwrap();
            assert!(sub.contains(venue.sub), "{sub}");
        }
    }

    #[cfg(feature = "extended")]
    #[tokio::test]
    async fn extended_ws_messages_point_to_the_stream_url() {
        let client = PerpDexClients::Extended(ExtendedCli::default());
        let channel = WsChannel::Lob(Some(LobParam::Bbo { frequency: None }));

        assert!(matches!(
            client.get_public_connect_msg(&channel).await,
            Err(InfraError::ApiCliError(_))
        ));
        assert!(matches!(
            client.get_public_connect_target(&channel).await,
            Err(InfraError::ApiCliError(_))
        ));
        assert!(matches!(
            client
                .get_public_sub_msg(&channel, Some(&["NVDA_24_5_USD_PERP".to_string()]))
                .await,
            Err(InfraError::ApiCliError(_))
        ));
    }

    #[tokio::test]
    async fn private_ws_is_unimplemented_for_public_only_venues() {
        for client in all_clients().iter().skip(1) {
            assert!(matches!(
                client
                    .get_private_connect_msg(&WsChannel::AccountOrders)
                    .await,
                Err(InfraError::Unimplemented)
            ));
            assert!(matches!(
                client.get_private_sub_msg(&WsChannel::AccountOrders).await,
                Err(InfraError::Unimplemented)
            ));
        }
    }

    #[tokio::test]
    async fn candles_are_unimplemented_for_the_new_venues() {
        for client in all_clients().iter().skip(1) {
            assert!(matches!(
                client
                    .get_candles(
                        "x",
                        InstrumentType::Perpetual,
                        CandleParam::OneMinute,
                        None,
                        None,
                        None
                    )
                    .await,
                Err(InfraError::Unimplemented)
            ));
        }
    }
}
