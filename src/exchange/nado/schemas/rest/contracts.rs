use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::price_data::MarkPriceData, base_data::InstrumentType,
};

use crate::exchange::nado::api_utils::nado_product_to_cli;

/// Archive perp contract summary, keyed by `ticker_id`.
#[derive(Clone, Debug, Deserialize)]
pub struct RestContractNado {
    pub product_id: u32,
    pub ticker_id: String,
    pub product_type: String,
    pub last_price: f64,
    pub index_price: f64,
    pub mark_price: f64,
    pub funding_rate: f64,
    pub next_funding_rate_timestamp: u64,
    pub open_interest: f64,
}

impl RestContractNado {
    pub fn inst(&self) -> String {
        nado_product_to_cli(self.product_id)
    }

    pub fn inst_type(&self) -> InstrumentType {
        match self.product_type.as_str() {
            "perpetual" => InstrumentType::Perpetual,
            _ => InstrumentType::Unknown,
        }
    }

    pub fn into_mark_price_data(self, timestamp: u64) -> MarkPriceData {
        MarkPriceData {
            timestamp,
            inst: self.inst(),
            inst_type: self.inst_type(),
            mark_price: self.mark_price,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_maps_to_mark_price() {
        let raw = r#"{"product_id":112,"ticker_id":"NVDA-PERP_USDT0","base_currency":"NVDA-PERP",
            "quote_currency":"USDT0","last_price":223.25,"base_volume":451.43,"quote_volume":101114.104899171,
            "product_type":"perpetual","contract_price":223.29502107382717,"contract_price_currency":"USD",
            "open_interest":4051.26,"open_interest_usd":904626.1870755531,"index_price":223.33424050347944,
            "mark_price":223.31274266135253,"funding_rate":-9.6628626229314e-05,
            "next_funding_rate_timestamp":1790586000,"price_change_percent_24h":-0.79837218901556}"#;

        let contract: RestContractNado = serde_json::from_str(raw).unwrap();
        assert_eq!(contract.next_funding_rate_timestamp, 1_790_586_000);
        assert_eq!(contract.index_price, 223.33424050347944);

        let mark = contract.into_mark_price_data(5);
        assert_eq!(mark.inst, "@112");
        assert_eq!(mark.inst_type, InstrumentType::Perpetual);
        assert_eq!(mark.mark_price, 223.31274266135253);
        assert_eq!(mark.timestamp, 5);
    }
}
