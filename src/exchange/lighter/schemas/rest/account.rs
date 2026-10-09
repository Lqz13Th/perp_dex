use serde::Deserialize;

use extrema_infra::arch::market_assets::{
    api_data::account_data::{BalanceData, PositionData},
    base_data::{InstrumentType, PositionSide},
};

use crate::exchange::lighter::api_utils::lighter_market_to_cli;

/// `GET /api/v1/account?by=index&value=<account index>`.
#[derive(Clone, Debug, Deserialize)]
pub struct RestAccountsLighter {
    #[serde(default)]
    pub accounts: Vec<AccountLighter>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AccountLighter {
    pub index: i64,
    pub collateral: String,
    pub available_balance: String,
    #[serde(default)]
    pub positions: Vec<AccountPositionLighter>,
}

/// One market of the account; `position` is unsigned, `sign` is 1 (long) or -1 (short).
#[derive(Clone, Debug, Deserialize)]
pub struct AccountPositionLighter {
    pub market_id: u16,
    pub symbol: String,
    pub sign: i8,
    pub position: String,
    pub avg_entry_price: String,
    pub position_value: String,
    #[serde(default)]
    pub allocated_margin: String,
    /// percent: "20.00" = 5x
    pub initial_margin_fraction: String,
    /// 0 cross, 1 isolated
    #[serde(default)]
    pub margin_mode: u8,
    #[serde(default)]
    pub liquidation_price: String,
}

fn num(s: &str) -> f64 {
    s.parse().unwrap_or_default()
}

impl AccountLighter {
    pub fn into_balance_data(&self, timestamp: u64) -> BalanceData {
        let (total, available) = (num(&self.collateral), num(&self.available_balance));
        BalanceData {
            timestamp,
            asset: "USDC".into(),
            total,
            frozen: (total - available).max(0.0),
            available,
            borrowed: None,
        }
    }
}

impl AccountPositionLighter {
    pub fn size(&self) -> f64 {
        f64::from(self.sign.signum()) * num(&self.position)
    }

    pub fn into_position_data(&self, timestamp: u64) -> PositionData {
        let size = self.size();
        let imf = num(&self.initial_margin_fraction);
        PositionData {
            timestamp,
            inst: lighter_market_to_cli(self.market_id),
            inst_type: InstrumentType::Perpetual,
            position_side: if size > 0.0 {
                PositionSide::Long
            } else if size < 0.0 {
                PositionSide::Short
            } else {
                PositionSide::Unknown
            },
            size,
            avg_price: num(&self.avg_entry_price),
            mark_price: if size != 0.0 {
                num(&self.position_value).abs() / size.abs()
            } else {
                0.0
            },
            margin: num(&self.allocated_margin),
            leverage: if imf > 0.0 { 100.0 / imf } else { 0.0 },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_positions_and_balance_map() {
        let raw = r#"{"code":200,"total":1,"accounts":[{"index":758666,"collateral":"28.001932",
            "available_balance":"20.5","positions":[
            {"market_id":139,"symbol":"SNDK","initial_margin_fraction":"20.00","sign":-1,"position":"0.0072",
             "avg_entry_price":"1630.00","position_value":"-11.736","margin_mode":1,"allocated_margin":"2.40",
             "liquidation_price":"1900"},
            {"market_id":1,"symbol":"BTC","initial_margin_fraction":"2.00","sign":1,"position":"0.0000",
             "avg_entry_price":"0","position_value":"0","margin_mode":0}]}]}"#;
        let a: RestAccountsLighter = serde_json::from_str(raw).unwrap();
        let acc = &a.accounts[0];
        let b = acc.into_balance_data(1);
        assert!((b.total - 28.001932).abs() < 1e-9 && (b.frozen - 7.501932).abs() < 1e-9);
        let p = acc.positions[0].into_position_data(1);
        assert_eq!(p.inst, "@139");
        assert!((p.size + 0.0072).abs() < 1e-12);
        assert_eq!(p.position_side, PositionSide::Short);
        assert!((p.mark_price - 1630.0).abs() < 1e-9 && (p.leverage - 5.0).abs() < 1e-9);
        assert_eq!(acc.positions[1].size(), 0.0);
    }
}
