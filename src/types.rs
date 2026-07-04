use tokio::sync::oneshot;

use crate::{ClientId, Order, OrderId, Timestamp, book::BookError};

pub type Price = u64; // in ticks
pub type Qty = u64; // in lots
pub type TradeId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug)]
pub enum PriceError {
    InvalidFormat,
    Overflow,
    TooManyDecimals,
}

#[derive(Debug, Clone)]
pub struct Trade {
    pub id: TradeId,
    pub buyer_order_id: OrderId,
    pub seller_order_id: OrderId,
    pub buyer_client_id: ClientId,
    pub seller_client_id: ClientId,
    pub price: Price,
    pub qty: Qty,
    pub timestamp: Timestamp,
}

#[derive(Debug)]
pub enum Command {
    PlaceOrder {
        order: Order,
        response: oneshot::Sender<CommandResult>,
    },
    CancelOrder {
        id: OrderId,
        response: oneshot::Sender<CommandResult>,
    },
}

#[derive(Debug)]
pub enum CommandResult {
    Placed { trades: Vec<Trade> },
    Canceled { order: Order },
    Error(BookError),
}

#[derive(Debug, Clone)]
pub enum Event {
    OrderPlaced { order: Order },
    OrderCanceled { order_id: OrderId },
    Trade { trade: Trade },
}

pub fn parse_price(s: &str, tick_decimals: u32) -> Result<Price, PriceError> {
    let (whole_str, fraction_str) = s.split_once(".").unwrap_or((s, ""));

    if whole_str.is_empty() || (fraction_str.is_empty() && s.contains('.')) {
        return Err(PriceError::InvalidFormat);
    }

    let whole_int = whole_str
        .parse::<Price>()
        .map_err(|_| PriceError::InvalidFormat)?;

    let fraction_len = fraction_str.len();

    let fraction_int = if fraction_str.is_empty() {
        0
    } else {
        fraction_str
            .parse::<Price>()
            .map_err(|_| PriceError::InvalidFormat)?
    };

    if (tick_decimals as usize) < fraction_str.len() {
        return Err(PriceError::TooManyDecimals);
    }

    let fraction = fraction_int
        .checked_mul(
            10u64
                .checked_pow(
                    tick_decimals
                        .checked_sub(fraction_len.try_into().map_err(|_| PriceError::Overflow)?)
                        .ok_or(PriceError::Overflow)?,
                )
                .ok_or(PriceError::Overflow)?,
        )
        .ok_or(PriceError::Overflow)?;

    Ok((whole_int
        .checked_mul(
            10u64
                .checked_pow(tick_decimals)
                .ok_or(PriceError::Overflow)?,
        )
        .ok_or(PriceError::Overflow)?)
    .checked_add(fraction)
    .ok_or(PriceError::Overflow)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parsing() {
        assert!(parse_price("abc", 2).is_err());
        assert!(parse_price("", 2).is_err());
        assert!(parse_price("1.2.3", 2).is_err());
        assert!(parse_price("103.", 2).is_err());
        assert!(parse_price(".5", 2).is_err());
        assert_eq!(parse_price("103.05", 4).unwrap(), 1030500); // the padding bug
        assert_eq!(parse_price("103.5", 4).unwrap(), 1035000);
        assert_eq!(parse_price("103", 2).unwrap(), 10300);
        assert!(parse_price("103.579", 2).is_err()); // too precise
        assert!(parse_price("999999999999999999999", 2).is_err()); // overflow
    }
}
