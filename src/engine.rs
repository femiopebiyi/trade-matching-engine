use tokio::sync::{broadcast, mpsc};

use crate::{Command, CommandResult, Event, OrderBook, Trade};

pub struct Engine {
    book: OrderBook,
    commands: mpsc::Receiver<Command>,
    events: broadcast::Sender<Event>,
    recent_trades: Vec<Trade>, // append-only log
}

impl Engine {
    pub fn new(
        book: OrderBook,
        commands: mpsc::Receiver<Command>,
        events: broadcast::Sender<Event>,
    ) -> Self {
        Self {
            book,
            commands,
            events,
            recent_trades: Vec::new(),
        }
    }

    pub async fn run(mut self) {
        while let Some(cmd) = self.commands.recv().await {
            self.handle(cmd);
        }
    }

    fn handle(&mut self, cmd: Command) {
        // Match on cmd. For PlaceOrder: call self.book.execute(order),
        //   emit an OrderPlaced event if it rested (i.e. remaining_qty > 0
        //   after matching AND it was a limit — actually the book already
        //   handled the resting; you need a different way to detect it. See note.)
        //   Emit a Trade event for each trade in the returned vec.
        //   Send the result through the oneshot.
        // For CancelOrder: call self.book.cancel(id), emit an OrderCanceled
        //   event on success, send the result through the oneshot.

        match cmd {
            Command::PlaceOrder { order, response } => {
                let order_id = order.id;
                let order_clone = order.clone();
                let _ = self
                    .events
                    .send(Event::OrderReceived { order: order_clone });
                let result = self.book.execute(order);

                if let Some(resting) = result.resting_order {
                    let _ = self.events.send(Event::OrderPlaced { order: resting });
                }

                for trade in &result.trades {
                    self.recent_trades.push(trade.clone());
                    let _ = self.events.send(Event::Trade {
                        trade: trade.clone(),
                    });
                }

                let _ = response.send(CommandResult::Placed {
                    order_id,
                    trades: result.trades,
                });
            }

            Command::CancelOrder { id, response } => match self.book.cancel(id) {
                Ok(cancelled) => {
                    let _ = self.events.send(Event::OrderCanceled { order_id: id });
                    let _ = response.send(CommandResult::Canceled { order: cancelled });
                }
                Err(e) => {
                    let _ = response.send(CommandResult::Error(e));
                }
            },

            Command::GetBook { response } => {
                let snapshot = self.book.snapshot();
                let _ = response.send(CommandResult::Book(snapshot));
            }

            Command::GetTrades { response } => {
                let _ = response.send(CommandResult::Trades(self.recent_trades.clone()));
            }
        }
    }
}
