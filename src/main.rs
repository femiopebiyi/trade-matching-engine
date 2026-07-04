use matching_engine::*;
use tokio::sync::{broadcast, mpsc, oneshot};

#[tokio::main]
async fn main() {
    // Create channels
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>(1024);
    let (evt_tx, mut evt_rx) = broadcast::channel::<Event>(1024);

    // Create the book and engine
    let book = OrderBook::new("EUR/USD".to_string(), 2);
    let engine = Engine::new(book, cmd_rx, evt_tx.clone());

    // Spawn the engine task
    tokio::spawn(engine.run());

    // Spawn a listener that prints every event
    tokio::spawn(async move {
        while let Ok(evt) = evt_rx.recv().await {
            println!("EVENT: {:?}", evt);
        }
    });

    // Command 1: place a sell limit — should rest (no buyers yet)
    let (r1, rx1) = oneshot::channel();
    cmd_tx
        .send(Command::PlaceOrder {
            order: Order::new(1, 12, Side::Sell, OrderType::Limit, 100, 10),
            response: r1,
        })
        .await
        .unwrap();
    println!("RESULT 1: {:?}", rx1.await);

    // Command 2: place a buy limit at same price — should match
    let (r2, rx2) = oneshot::channel();
    cmd_tx
        .send(Command::PlaceOrder {
            order: Order::new(2, 13, Side::Buy, OrderType::Limit, 100, 10),
            response: r2,
        })
        .await
        .unwrap();
    println!("RESULT 2: {:?}", rx2.await);

    // Give the event listener a moment to print
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}
