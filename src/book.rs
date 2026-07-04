use std::{
    cmp::min,
    collections::{BTreeMap, HashMap, VecDeque},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{Order, OrderId, OrderType, Price, Side, Trade, TradeId};

pub struct OrderBook {
    pub symbol: String,
    tick_decimals: u32,
    bids: BTreeMap<Price, VecDeque<Order>>,
    asks: BTreeMap<Price, VecDeque<Order>>,
    // Auxiliary index — see below.
    order_index: std::collections::HashMap<OrderId, (Side, Price)>,
    next_trade_id: TradeId,
}

#[derive(Debug, PartialEq)]
pub enum BookError {
    DuplicateOrderId,
    OrderNotFound,
    MarketOrderNotAllowed, // in this task; market orders can't rest
    PriceMisaligned,       // price doesn't fit the tick grid
}

#[derive(Debug)]
pub struct ExecuteResult {
    pub trades: Vec<Trade>,
    pub resting_order: Option<Order>,
}

impl OrderBook {
    pub fn new(symbol: String, tick_decimals: u32) -> Self {
        Self {
            symbol,
            tick_decimals,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            order_index: HashMap::new(),
            next_trade_id: 1,
        }
    }

    pub fn add(&mut self, order: Order) -> Result<(), BookError> {
        if self.order_index.contains_key(&order.id) {
            return Err(BookError::DuplicateOrderId);
        }

        if order.order_type == OrderType::Market {
            return Err(BookError::MarketOrderNotAllowed);
        }

        match order.side {
            Side::Buy => {
                let bids = self.bids.entry(order.price).or_insert(VecDeque::new());
                self.order_index.insert(order.id, (Side::Buy, order.price));
                bids.push_back(order);
            }

            Side::Sell => {
                let asks = self.asks.entry(order.price).or_insert(VecDeque::new());
                self.order_index.insert(order.id, (Side::Sell, order.price));
                asks.push_back(order);
            }
        }

        Ok(())
    }

    pub fn cancel(&mut self, id: OrderId) -> Result<Order, BookError> {
        let &(side, price) = self.order_index.get(&id).ok_or(BookError::OrderNotFound)?;

        let vec = self.side_map(side).get_mut(&price).unwrap();
        if let Some(pos) = vec.iter().position(|x| x.id == id) {
            let removed = vec.remove(pos).ok_or(BookError::OrderNotFound)?;

            if vec.is_empty() {
                self.side_map(side).remove(&price);
            }

            self.order_index.remove(&id);
            Ok(removed)
        } else {
            Err(BookError::OrderNotFound)
        }
    }

    fn side_map(&mut self, side: Side) -> &mut BTreeMap<Price, VecDeque<Order>> {
        match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        }
    }

    pub fn best_bid(&self) -> Option<Price> {
        self.bids.last_key_value().map(|(&price, _)| price)
    }

    pub fn best_ask(&self) -> Option<Price> {
        self.asks.first_key_value().map(|(&price, _)| price)
    }

    fn _opposite_side_map(&mut self, side: Side) -> &mut BTreeMap<Price, VecDeque<Order>> {
        match side {
            Side::Buy => &mut self.asks,
            Side::Sell => &mut self.bids,
        }
    }

    pub fn execute(&mut self, mut order: Order) -> ExecuteResult {
        let mut trades = Vec::new();

        // THE MAIN LOOP: keep matching until the incoming order is empty
        // or the book stops crossing.
        while order.remaining_qty > 0 {
            // 1. Find the best price on the OPPOSITE side.
            //    Buy matches asks; Sell matches bids.
            //    If that side is empty → break (nothing to match against).
            let best_price = match order.side {
                Side::Buy => self.asks.first_key_value().map(|(&p, _)| p),
                Side::Sell => self.bids.last_key_value().map(|(&p, _)| p),
            };
            let best_price = match best_price {
                Some(p) => p,
                None => break, // opposite side empty
            };

            // 2. Does it cross?
            //    Buy crosses if order.price >= best_price.
            //    Sell crosses if order.price <= best_price.
            //    Market orders ALWAYS cross (skip the price check).
            let crosses = match order.order_type {
                OrderType::Market => true,
                OrderType::Limit => match order.side {
                    Side::Buy => order.price >= best_price,
                    Side::Sell => order.price <= best_price,
                },
            };
            if !crosses {
                break; // spread not crossed, no more trades
            }

            // 3. Match against the FIFO queue at best_price.
            //    Get the queue (on the opposite side, at best_price).
            //    Walk it front-to-back:
            //      - trade_qty = min(order.remaining_qty, resting.remaining_qty)
            //      - decrement both remaining_qty
            //      - push a Trade
            //      - if resting fully filled: pop it, remove from order_index
            //      - if incoming fully filled: stop walking this queue
            //    (this is the inner loop — write it next)

            let queue = match order.side {
                Side::Buy => self.asks.get_mut(&best_price).unwrap(),
                Side::Sell => self.bids.get_mut(&best_price).unwrap(),
            };
            while order.remaining_qty > 0 {
                let resting = match queue.front_mut() {
                    Some(r) => r,
                    None => break, // queue emptied
                };

                let (buyer_order_id, seller_order_id, buyer_client_id, seller_client_id) =
                    match order.side {
                        Side::Buy => (order.id, resting.id, order.client_id, resting.client_id),
                        Side::Sell => (resting.id, order.id, resting.client_id, order.client_id),
                    };

                let trade_qty = min(order.remaining_qty, resting.remaining_qty);
                order.remaining_qty -= trade_qty;
                resting.remaining_qty -= trade_qty;
                trades.push(Trade {
                    id: self.next_trade_id,
                    buyer_order_id: buyer_order_id,
                    seller_order_id: seller_order_id,
                    buyer_client_id: buyer_client_id,
                    seller_client_id: seller_client_id,
                    price: best_price,
                    qty: trade_qty,
                    timestamp: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .expect("system time before UNIX epoch")
                        .as_nanos() as u64,
                });

                self.next_trade_id += 1;

                if resting.remaining_qty == 0 {
                    let filled = queue.pop_front().unwrap();
                    let order_index = &mut self.order_index;
                    order_index.remove(&filled.id);
                }
            }

            let queue_empty = match order.side {
                Side::Buy => self.asks.get(&best_price).map_or(true, |q| q.is_empty()),
                Side::Sell => self.bids.get(&best_price).map_or(true, |q| q.is_empty()),
            };

            if queue_empty {
                match order.side {
                    Side::Buy => self.asks.remove(&best_price),
                    Side::Sell => self.bids.remove(&best_price),
                };
            }

            // 4. If the queue at best_price is now empty, remove the price key
            //    from the opposite side's BTreeMap.
        }

        // 5. After the loop: if the order is a Limit with remaining_qty > 0,
        //    it rests. Call self.add(order). (Market orders just vanish.)
        let resting_order = if order.remaining_qty > 0 && order.order_type == OrderType::Limit {
            let resting = order.clone();
            let _ = self.add(order);
            Some(resting)
        } else {
            None
        };

        ExecuteResult {
            trades,
            resting_order,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_inspect_best_prices() {
        // Add a bid at 100 and an ask at 101. Best bid = 100, best ask = 101.
        let mut book = OrderBook::new("EUR/USD".to_string(), 2);
        let order_bid = Order::new(1, 12, Side::Buy, OrderType::Limit, 100, 50);
        let order_ask = Order::new(2, 12, Side::Sell, OrderType::Limit, 101, 50);

        book.add(order_bid).unwrap();
        book.add(order_ask).unwrap();

        assert_eq!(book.best_bid().unwrap(), 100);
        assert_eq!(book.best_ask().unwrap(), 101);
    }

    #[test]
    fn multiple_orders_same_price_preserve_fifo() {
        // Add two bids at 100. Cancel the first. The second should still be there.
        // Best bid still 100.
        let mut book = OrderBook::new("EUR/USD".to_string(), 2);
        let order1 = Order::new(1, 12, Side::Buy, OrderType::Limit, 100, 101);
        let order2 = Order::new(2, 12, Side::Buy, OrderType::Limit, 100, 101);

        let (order1_id, order2_id) = (order1.id, order2.id);

        book.add(order1).unwrap();
        book.add(order2).unwrap();

        book.cancel(order1_id).unwrap();

        assert_eq!(book.best_bid().unwrap(), 100);
        assert!(book.order_index.contains_key(&order2_id));
    }

    #[test]
    fn cancel_removes_from_index() {
        // Add, cancel, then try to cancel again — should error OrderNotFound.

        let mut book = OrderBook::new("EUR/USD".to_string(), 2);
        let order = Order::new(1, 12, Side::Buy, OrderType::Limit, 100, 101);
        let order_id = order.id;

        book.add(order).unwrap();
        book.cancel(order_id).unwrap();

        assert_eq!(book.cancel(order_id), Err(BookError::OrderNotFound));
    }

    #[test]
    fn duplicate_id_rejected() {
        // Add order id 1. Add another order id 1. Should error.
        let mut book = OrderBook::new("EUR/USD".to_string(), 2);
        let order1 = Order::new(1, 12, Side::Buy, OrderType::Limit, 100, 101);
        let order2 = Order::new(1, 12, Side::Buy, OrderType::Limit, 100, 101);

        book.add(order1).unwrap();

        assert_eq!(book.add(order2), Err(BookError::DuplicateOrderId));
    }

    #[test]
    fn empty_price_level_disappears() {
        // Add a bid at 100. Cancel it. best_bid() should return None.
        // (Otherwise the BTreeMap grows forever with dead empty VecDeques.)
        let mut book = OrderBook::new("EUR/USD".to_string(), 2);
        let order = Order::new(1, 12, Side::Buy, OrderType::Limit, 100, 101);
        let order_id = order.id;

        book.add(order).unwrap();
        book.cancel(order_id).unwrap();
        assert_eq!(book.best_bid(), None)
    }

    #[test]
    fn market_order_cannot_rest() {
        // add() with OrderType::Market should error.

        let mut book = OrderBook::new("EUR/USD".to_string(), 2);
        let order = Order::new(1, 12, Side::Buy, OrderType::Market, 100, 101);

        assert_eq!(book.add(order), Err(BookError::MarketOrderNotAllowed));
    }

    //Match book tests

    #[test]
    fn limit_buy_crosses_resting_ask() {
        // Ask resting at 100, qty 10. Buy comes in at 100, qty 10.
        // One trade: price 100, qty 10. Book is now empty.
        let mut book = OrderBook::new("EUR/USD".to_string(), 2);
        let order1 = Order::new(1, 12, Side::Sell, OrderType::Limit, 100, 10);
        let order2 = Order::new(2, 13, Side::Buy, OrderType::Limit, 100, 10);

        let trade1 = book.execute(order1);
        let trade2 = book.execute(order2);

        println!("{:?}", trade1);
        println!("{:?}", trade2);

        assert!(book.asks.is_empty() && book.bids.is_empty());
    }

    #[test]
    fn partial_fill_incoming_rests() {
        // Ask at 100, qty 5. Buy at 100, qty 10.
        // Trade: price 100, qty 5. Buy rests with remaining 5.
        // best_bid() == 100, best_ask() == None.
        let mut book = OrderBook::new("EUR/USD".to_string(), 2);
        let order1 = Order::new(1, 12, Side::Sell, OrderType::Limit, 100, 5);
        let order2 = Order::new(2, 13, Side::Buy, OrderType::Limit, 100, 10);

        let trade1 = book.execute(order1);
        let trade2 = book.execute(order2);

        println!("{:?}", trade1);
        println!("{:?}", trade2);

        assert_eq!(book.best_bid().unwrap(), 100);
        assert_eq!(book.best_ask(), None);
        assert_eq!(trade2.trades.len(), 1);
        assert_eq!(trade2.trades[0].qty, 5);
        assert_eq!(trade2.trades[0].price, 100);
    }

    #[test]
    fn incoming_walks_multiple_levels() {
        // Ask 5 @ 100, Ask 5 @ 101. Buy 10 @ 101.
        // Two trades: 5 @ 100, 5 @ 101. Book empty.
        let mut book = OrderBook::new("EUR/USD".to_string(), 2);
        let order1 = Order::new(1, 12, Side::Sell, OrderType::Limit, 100, 5);
        let order2 = Order::new(2, 13, Side::Sell, OrderType::Limit, 101, 5);
        let order3 = Order::new(3, 14, Side::Buy, OrderType::Limit, 101, 10);

        let trade1 = book.execute(order1);
        let trade2 = book.execute(order2);
        let trade3 = book.execute(order3);

        println!("{:#?}", trade1);
        println!("{:#?}", trade2);
        println!("{:#?}", trade3);

        assert!(book.asks.is_empty());
        assert!(book.bids.is_empty());
        assert_eq!(trade3.trades.len(), 2);
    }

    #[test]
    fn no_cross_no_trade() {
        // Ask at 105. Buy at 100. No trades. Both rest.
        // best_bid() == 100, best_ask() == 105.

        let mut book = OrderBook::new("EUR/USD".to_string(), 2);
        let order1 = Order::new(1, 12, Side::Sell, OrderType::Limit, 105, 5);
        let order2 = Order::new(2, 13, Side::Buy, OrderType::Limit, 100, 5);

        let trade1 = book.execute(order1);
        let trade2 = book.execute(order2);

        println!("{:#?}", trade1);
        println!("{:#?}", trade2);

        assert_eq!(book.best_ask(), Some(105));
        assert_eq!(book.best_bid(), Some(100));
        assert!(trade1.trades.is_empty() && trade2.trades.is_empty());
    }

    #[test]
    fn market_order_partial_fill_does_not_rest() {
        // Ask 5 @ 100. Market buy qty 10.
        // Trade: 5 @ 100. No resting order. best_bid() == None.

        let mut book = OrderBook::new("EUR/USD".to_string(), 2);
        let order1 = Order::new(1, 12, Side::Sell, OrderType::Limit, 100, 5);
        let order2 = Order::new(2, 13, Side::Buy, OrderType::Market, 100, 10);

        let trade1 = book.execute(order1);
        let trade2 = book.execute(order2);

        println!("{:#?}", trade1);
        println!("{:#?}", trade2);

        assert_eq!(book.best_bid(), None);
        assert_eq!(trade2.trades.len(), 1);
        assert_eq!(trade2.trades[0].qty, 5);
    }

    #[test]
    fn market_order_fills_immediately() {
        // Ask 10 @ 100. Market buy qty 10.
        // Trade: 10 @ 100. No resting order. best_bid() == None.
        let mut book = OrderBook::new("EUR/USD".to_string(), 2);
        let order1 = Order::new(1, 12, Side::Sell, OrderType::Limit, 106, 10);
        let order2 = Order::new(2, 13, Side::Buy, OrderType::Market, 100, 10);

        let trade1 = book.execute(order1);
        let trade2 = book.execute(order2);

        println!("{:#?}", trade1);
        println!("{:#?}", trade2);

        assert_eq!(book.best_bid(), None);
        assert_eq!(trade2.trades.len(), 1);
        assert_eq!(trade2.trades[0].qty, 10)
    }

    #[test]
    fn price_time_priority() {
        // Ask 5 @ 100 (order A, placed first). Ask 5 @ 100 (order B, placed second).
        // Buy 5 @ 100.
        // Trade should be against order A, not B.
        // Order B still resting.
        let mut book = OrderBook::new("EUR/USD".to_string(), 2);
        let order_a = Order::new(1, 12, Side::Sell, OrderType::Limit, 100, 5);
        let order_b = Order::new(2, 13, Side::Sell, OrderType::Limit, 100, 5);
        let order_c = Order::new(3, 14, Side::Buy, OrderType::Limit, 100, 5);

        let trade1 = book.execute(order_a);
        let trade2 = book.execute(order_b);
        let trade3 = book.execute(order_c);

        println!("{:#?}", trade1);
        println!("{:#?}", trade2);
        println!("{:#?}", trade3);

        assert_eq!(trade3.trades[0].seller_client_id, 12);
        assert!(book.order_index.contains_key(&2)); // Order B still resting
        assert!(!book.order_index.contains_key(&1)); // Order A gone
    }
}
