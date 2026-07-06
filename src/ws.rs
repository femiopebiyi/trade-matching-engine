use axum::{
    extract::ws::{Message, WebSocket},
    extract::{State, WebSocketUpgrade},
    response::Response,
};
use std::sync::Arc;
use tokio::sync::{broadcast, oneshot};

use crate::{AppState, BookLevelResponse, Command, CommandResult, Event, WsMessage, format_price};

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    let events_rx = state.evt_tx.subscribe();
    ws.on_upgrade(move |socket| handle_socket(socket, state, events_rx))
}

async fn handle_socket(
    mut socket: WebSocket,
    state: Arc<AppState>,
    mut events_rx: broadcast::Receiver<Event>,
) {
    // 1. Send initial book snapshot
    if let Some(snapshot) = fetch_snapshot(&state).await {
        let msg = WsMessage::Snapshot {
            bids: snapshot.bids,
            asks: snapshot.asks,
        };
        if let Ok(json) = serde_json::to_string(&msg) {
            if socket.send(Message::Text(json.into())).await.is_err() {
                return; // client disconnected
            }
        }
    }

    // 2. Forward events as they arrive
    loop {
        match events_rx.recv().await {
            Ok(event) => {
                let msgs = event_to_ws_messages(event, state.tick_decimals);
                for msg in msgs {
                    if let Ok(json) = serde_json::to_string(&msg) {
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            return; // client disconnected
                        }
                    }
                }
            }
            Err(broadcast::error::RecvError::Lagged(n)) => {
                // Subscriber fell behind — send a fresh snapshot to resync
                eprintln!("ws client lagged, missed {} events, resyncing", n);
                if let Some(snapshot) = fetch_snapshot(&state).await {
                    let msg = WsMessage::Snapshot {
                        bids: snapshot.bids,
                        asks: snapshot.asks,
                    };
                    if let Ok(json) = serde_json::to_string(&msg) {
                        if socket.send(Message::Text(json.into())).await.is_err() {
                            return;
                        }
                    }
                }
            }
            Err(broadcast::error::RecvError::Closed) => {
                return; // engine shut down
            }
        }
    }
}

async fn fetch_snapshot(state: &Arc<AppState>) -> Option<crate::BookSnapshotResponse> {
    let (tx, rx) = oneshot::channel();
    state
        .cmd_tx
        .send(Command::GetBook { response: tx })
        .await
        .ok()?;
    match rx.await.ok()? {
        CommandResult::Book(snapshot) => Some(crate::BookSnapshotResponse {
            symbol: snapshot.symbol,
            bids: snapshot
                .bids
                .iter()
                .map(|l| BookLevelResponse {
                    price: format_price(l.price, state.tick_decimals),
                    total_qty: l.total_qty,
                    order_count: l.order_count,
                })
                .collect(),
            asks: snapshot
                .asks
                .iter()
                .map(|l| BookLevelResponse {
                    price: format_price(l.price, state.tick_decimals),
                    total_qty: l.total_qty,
                    order_count: l.order_count,
                })
                .collect(),
        }),
        _ => None,
    }
}

fn event_to_ws_messages(event: Event, tick_decimals: u32) -> Vec<WsMessage> {
    match event {
        Event::Trade { trade } => {
            vec![WsMessage::Trade {
                id: trade.id,
                buyer_order_id: trade.buyer_order_id,
                seller_order_id: trade.seller_order_id,
                price: format_price(trade.price, tick_decimals),
                qty: trade.qty,
            }]
        }
        Event::OrderPlaced { order } => {
            // A new order rested — the book changed at this price level.
            // We don't have the full level info here (total qty, count),
            // so we send a simple notification. The client can refetch
            // or we can enhance this later.
            vec![WsMessage::BookUpdate {
                side: order.side,
                price: format_price(order.price, tick_decimals),
                total_qty: order.remaining_qty,
                order_count: 1,
            }]
        }
        Event::OrderCanceled { .. } => {
            // A cancel changes the book too, but we don't have the
            // price/side info in this event. For now, skip it.
            // The client can poll /book to resync. We'll improve this later.
            vec![]
        }
        Event::OrderReceived { .. } => {
            // Not relevant for WS clients — the writer cares, not the UI.
            vec![]
        }
    }
}
