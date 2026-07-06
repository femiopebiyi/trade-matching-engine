CREATE TABLE IF NOT EXISTS orders (
    id          BIGINT PRIMARY KEY,
    client_id   BIGINT NOT NULL,
    side        TEXT NOT NULL CHECK (side IN ('Buy', 'Sell')),
    order_type  TEXT NOT NULL CHECK (order_type IN ('Limit', 'Market')),
    price       BIGINT NOT NULL,
    qty         BIGINT NOT NULL,
    remaining_qty BIGINT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'partial', 'filled', 'canceled')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS trades (
    id              BIGINT PRIMARY KEY,
    buyer_order_id  BIGINT NOT NULL REFERENCES orders(id),
    seller_order_id BIGINT NOT NULL REFERENCES orders(id),
    buyer_client_id BIGINT NOT NULL,
    seller_client_id BIGINT NOT NULL,
    price           BIGINT NOT NULL,
    qty             BIGINT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_trades_created ON trades(created_at DESC);
CREATE INDEX IF NOT EXISTS idx_orders_client ON orders(client_id, status);