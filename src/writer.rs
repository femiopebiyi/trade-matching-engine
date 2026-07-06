use crate::{Event, Order};
use sqlx::PgPool;
use tokio::sync::broadcast;

pub struct Writer {
    pool: PgPool,
    events: broadcast::Receiver<Event>,
}

impl Writer {
    pub fn new(pool: PgPool, events: broadcast::Receiver<Event>) -> Self {
        Self { pool, events }
    }

    pub async fn run(mut self) {
        while let Ok(event) = self.events.recv().await {
            self.handle(event).await;
        }
    }

    async fn insert_order(&self, order: &Order) {
        let side = format!("{:?}", order.side);
        let order_type = format!("{:?}", order.order_type);
        sqlx::query(
        "INSERT INTO orders (id, client_id, side, order_type, price, qty, remaining_qty, status)
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'open')
         ON CONFLICT (id) DO NOTHING"
    )
    .bind(order.id as i64)
    .bind(order.client_id as i64)
    .bind(&side)
    .bind(&order_type)
    .bind(order.price as i64)
    .bind(order.qty as i64)
    .bind(order.remaining_qty as i64)
    .execute(&self.pool)
    .await
    .ok();
    }

    async fn handle(&mut self, event: Event) {
        match event {
            Event::OrderPlaced { order } => {
                let side = format!("{:?}", order.side);
                let order_type = format!("{:?}", order.order_type);
                sqlx::query(
        "INSERT INTO orders (id, client_id, side, order_type, price, qty, remaining_qty, status)
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'open')
         ON CONFLICT (id) DO NOTHING"
    )
    .bind(order.id as i64)
    .bind(order.client_id as i64)
    .bind(&side)
    .bind(&order_type)
    .bind(order.price as i64)
    .bind(order.qty as i64)
    .bind(order.remaining_qty as i64)
    .execute(&self.pool)
    .await
    .ok();
            }
            Event::Trade { trade } => {
                // 1. Insert the trade
                sqlx::query(
        "INSERT INTO trades (id, buyer_order_id, seller_order_id, buyer_client_id, seller_client_id, price, qty)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         ON CONFLICT (id) DO NOTHING"
    )
    .bind(trade.id as i64)
    .bind(trade.buyer_order_id as i64)
    .bind(trade.seller_order_id as i64)
    .bind(trade.buyer_client_id as i64)
    .bind(trade.seller_client_id as i64)
    .bind(trade.price as i64)
    .bind(trade.qty as i64)
    .execute(&self.pool)
    .await
    .ok();

                // 2. Update buyer's order
                sqlx::query(
                    "UPDATE orders SET remaining_qty = remaining_qty - $1,
         status = CASE WHEN remaining_qty - $1 = 0 THEN 'filled' ELSE 'partial' END
         WHERE id = $2",
                )
                .bind(trade.qty as i64)
                .bind(trade.buyer_order_id as i64)
                .execute(&self.pool)
                .await
                .ok();

                // 3. Update seller's order
                sqlx::query(
                    "UPDATE orders SET remaining_qty = remaining_qty - $1,
         status = CASE WHEN remaining_qty - $1 = 0 THEN 'filled' ELSE 'partial' END
         WHERE id = $2",
                )
                .bind(trade.qty as i64)
                .bind(trade.seller_order_id as i64)
                .execute(&self.pool)
                .await
                .ok();
            }
            Event::OrderCanceled { order_id } => {
                sqlx::query("UPDATE orders SET status = 'canceled' WHERE id = $1")
                    .bind(order_id as i64)
                    .execute(&self.pool)
                    .await
                    .ok();
            }
            Event::OrderReceived { order } => {
                self.insert_order(&order).await;
            }
        }
    }
}
