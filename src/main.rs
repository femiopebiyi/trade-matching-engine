use std::sync::{Arc, atomic::AtomicU64};

use matching_engine::{AppState, Command, Engine, Event, OrderBook, Writer, router};
use sqlx::postgres::PgPoolOptions;
use tokio::sync::{broadcast, mpsc};

#[tokio::main]
async fn main() {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://localhost:5432/exchange".to_string());

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("failed to connect to database");

    // Run schema
    // sqlx::query(include_str!("../schema.sql"))
    //     .execute(&pool)
    //     .await
    //     .expect("failed to run schema");

    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(1024);
    let (evt_tx, _) = broadcast::channel::<Event>(1024);

    // Writer subscribes to events
    let writer = Writer::new(pool.clone(), evt_tx.subscribe());
    tokio::spawn(writer.run());

    let book = OrderBook::new("SOL/USDC".to_string(), 2);
    let engine = Engine::new(book, cmd_rx, evt_tx.clone());
    tokio::spawn(engine.run());

    let state = Arc::new(AppState {
        cmd_tx,
        tick_decimals: 2,
        next_order_id: AtomicU64::new(1),
    });
    let app = router(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("listening on :3000");
    axum::serve(listener, app).await.unwrap();
}
