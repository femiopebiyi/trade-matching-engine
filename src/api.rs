use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::{mpsc, oneshot};

use crate::{
    BookError, BookLevel, BookLevelResponse, BookSnapshot, BookSnapshotResponse, Command,
    CommandResult, Order, PlaceOrderRequest, PlaceOrderResponse, TradeResponse, format_price,
    parse_price,
};

pub struct AppState {
    pub cmd_tx: mpsc::Sender<Command>,
    pub tick_decimals: u32,
    pub next_order_id: AtomicU64,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/orders", post(place_order))
        .route("/orders/{id}", delete(cancel_order))
        .route("/book", get(get_book))
        .route("/trades", get(get_trades))
        .with_state(state)
}

async fn place_order(
    State(state): State<Arc<AppState>>,
    Json(req): Json<PlaceOrderRequest>,
) -> Result<Json<PlaceOrderResponse>, StatusCode> {
    let parsed_price =
        parse_price(&req.price, state.tick_decimals).map_err(|_| StatusCode::BAD_REQUEST)?;

    let id = state.next_order_id.fetch_add(1, Ordering::Relaxed);

    let order = Order::new(
        id,
        req.client_id,
        req.side,
        req.order_type,
        parsed_price,
        req.qty,
    );

    let (respond_to, response) = oneshot::channel();

    state
        .cmd_tx
        .send(Command::PlaceOrder {
            order,
            response: respond_to,
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let result = response
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Unpack the CommandResult
    match result {
        CommandResult::Placed { order_id, trades } => {
            let filled_qty: u64 = trades.iter().map(|t| t.qty).sum();
            let status = if filled_qty == req.qty {
                "filled"
            } else if filled_qty > 0 {
                "partial"
            } else {
                "resting"
            };

            let trade_responses: Vec<TradeResponse> = trades
                .iter()
                .map(|t| TradeResponse {
                    id: t.id,
                    buyer_order_id: t.buyer_order_id,
                    seller_order_id: t.seller_order_id,
                    price: format_price(t.price, state.tick_decimals),
                    qty: t.qty,
                })
                .collect();

            Ok(Json(PlaceOrderResponse {
                order_id,
                trades: trade_responses,
                status: status.to_string(),
            }))
        }
        CommandResult::Error(_) => Err(StatusCode::BAD_REQUEST),
        _ => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn cancel_order(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Send Command::CancelOrder, await, match on result
    let (respond_to, response) = oneshot::channel();

    state
        .cmd_tx
        .send(Command::CancelOrder {
            id,
            response: respond_to,
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let result = response
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        CommandResult::Canceled { order } => Ok(Json(json!(order))),

        CommandResult::Error(BookError::OrderNotFound) => Err(StatusCode::NOT_FOUND),
        CommandResult::Error(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
        _ => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_book(
    State(state): State<Arc<AppState>>,
) -> Result<Json<BookSnapshotResponse>, StatusCode> {
    // Send Command::GetBook, await, match on result
    let (respond_to, response) = oneshot::channel();

    state
        .cmd_tx
        .send(Command::GetBook {
            response: respond_to,
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let result = response
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        CommandResult::Book(snapshot) => {
            let formatted = BookSnapshotResponse {
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
            };
            Ok(Json(formatted))
        }
        CommandResult::Error(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
        _ => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

async fn get_trades(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<TradeResponse>>, StatusCode> {
    // Send Command::GetTrades, await, match on result
    // map each Trade to TradeResponse (format prices)
    let (respond_to, response) = oneshot::channel();

    state
        .cmd_tx
        .send(Command::GetTrades {
            response: respond_to,
        })
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let result = response
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match result {
        CommandResult::Trades(trades) => {
            let trade_lis = trades
                .iter()
                .map(|trade| TradeResponse {
                    buyer_order_id: trade.buyer_order_id,
                    id: trade.id,
                    price: format_price(trade.price, state.tick_decimals),
                    qty: trade.qty,
                    seller_order_id: trade.seller_order_id,
                })
                .collect::<Vec<TradeResponse>>();
            Ok(Json(trade_lis))
        }
        CommandResult::Error(_) => Err(StatusCode::INTERNAL_SERVER_ERROR),
        _ => Err(StatusCode::INTERNAL_SERVER_ERROR),
    }
}
