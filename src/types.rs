use thiserror::Error;
use tokio::sync::oneshot;

use crate::{ClientId, Order, OrderId, OrderType, Timestamp, book::BookError};

pub type Price = u64; // in ticks
pub type Qty = u64; // in lots
pub type TradeId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Error)]
pub enum PriceError {
    #[error("invalid price format")]
    InvalidFormat,
    #[error("price overflowed")]
    Overflow,
    #[error("too many decimal places for the configured tick size")]
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
    GetBook {
        response: oneshot::Sender<CommandResult>,
    },
    GetTrades {
        response: oneshot::Sender<CommandResult>,
    },
}

#[derive(Debug)]
pub enum CommandResult {
    Placed {
        order_id: OrderId,
        trades: Vec<Trade>,
    },
    Canceled {
        order: Order,
    },
    Book(BookSnapshot),
    Trades(Vec<Trade>),
    Error(BookError),
}

#[derive(Debug, Clone)]
pub enum Event {
    OrderReceived { order: Order },
    OrderPlaced { order: Order },
    OrderCanceled { order_id: OrderId },
    Trade { trade: Trade },
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct PlaceOrderRequest {
    pub client_id: ClientId,
    pub side: Side,            // "Buy" or "Sell"
    pub order_type: OrderType, // "Limit" or "Market"
    pub price: String,         // "103.57" — parsed via parse_price
    pub qty: Qty,
}

#[derive(Debug, Serialize)]
pub struct PlaceOrderResponse {
    pub order_id: OrderId,
    pub trades: Vec<TradeResponse>,
    pub status: String, // "filled", "partial", "resting"
}

#[derive(Debug, Serialize)]
pub struct TradeResponse {
    pub id: TradeId,
    pub buyer_order_id: OrderId,
    pub seller_order_id: OrderId,
    pub price: String, // formatted back from ticks
    pub qty: Qty,
}

#[derive(Debug, Serialize)]
pub struct BookLevel {
    pub price: Price,
    pub total_qty: Qty,
    pub order_count: usize,
}

#[derive(Debug, Serialize)]
pub struct BookLevelResponse {
    pub price: String,
    pub total_qty: Qty,
    pub order_count: usize,
}

#[derive(Debug, Serialize)]
pub struct BookSnapshotResponse {
    pub symbol: String,
    pub bids: Vec<BookLevelResponse>,
    pub asks: Vec<BookLevelResponse>,
}

#[derive(Debug, Serialize)]
pub struct BookSnapshot {
    pub symbol: String,
    pub bids: Vec<BookLevel>,
    pub asks: Vec<BookLevel>,
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

pub fn format_price(ticks: Price, tick_decimals: u32) -> String {
    let scale = 10u64.pow(tick_decimals);
    let whole = ticks / scale;
    let frac = ticks % scale;
    format!(
        "{}.{:0>width$}",
        whole,
        frac,
        width = tick_decimals as usize
    )
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
