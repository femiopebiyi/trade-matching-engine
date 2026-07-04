use std::time::{SystemTime, UNIX_EPOCH};

use crate::types::{Price, Qty, Side};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderType {
    Limit,
    Market,
}

pub type OrderId = u64;
pub type ClientId = u64;
pub type Timestamp = u64; // nanoseconds since UNIX epoch

#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    pub id: OrderId,
    pub client_id: ClientId,
    pub side: Side,
    pub order_type: OrderType,
    pub price: Price,       // in ticks; for Market orders, ignored
    pub qty: Qty,           // original quantity — never mutated
    pub remaining_qty: Qty, // decreases as fills happen
    pub timestamp: Timestamp,
}

impl Order {
    pub fn new(
        id: OrderId,
        client_id: ClientId,
        side: Side,
        order_type: OrderType,
        price: Price,
        qty: Qty,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_nanos() as u64;

        Self {
            id,
            client_id,
            side,
            order_type,
            price,
            qty,
            remaining_qty: qty,
            timestamp,
        }
    }

    pub fn filled_qty(&self) -> Qty {
        self.qty - self.remaining_qty
    }

    pub fn is_fully_filled(&self) -> bool {
        self.remaining_qty == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_order_has_full_remaining_qty() {
        let o = Order::new(1, 42, Side::Buy, OrderType::Limit, 10000, 5);
        assert_eq!(o.remaining_qty, 5);
        assert_eq!(o.filled_qty(), 0);
        assert!(!o.is_fully_filled());
    }

    #[test]
    fn two_orders_have_different_timestamps() {
        let a = Order::new(1, 1, Side::Buy, OrderType::Limit, 100, 1);
        std::thread::sleep(std::time::Duration::from_nanos(100));
        let b = Order::new(2, 1, Side::Buy, OrderType::Limit, 100, 1);
        assert!(b.timestamp > a.timestamp);
    }
}
