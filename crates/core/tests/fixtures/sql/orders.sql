CREATE SCHEMA IF NOT EXISTS shop;

-- cm: def orders
CREATE TABLE shop.orders (
    id          uuid PRIMARY KEY,
    customer_id uuid NOT NULL,
    status      text NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);

-- cm: def order_events
CREATE TABLE shop.order_events (
    order_id uuid NOT NULL REFERENCES shop.orders(id),
    kind     text NOT NULL,
    at       timestamptz NOT NULL DEFAULT now()
);

-- cm: def cancel_stale_orders
CREATE FUNCTION shop.cancel_stale_orders(p_before timestamptz)
RETURNS integer
LANGUAGE plpgsql AS $$
DECLARE
    v_count integer;
BEGIN
    WITH stale AS (
        SELECT id
        FROM shop.orders
        WHERE status = 'draft' AND created_at < p_before
    )
    UPDATE shop.orders o
    SET status = 'cancelled'
    FROM stale
    WHERE o.id = stale.id;

    GET DIAGNOSTICS v_count = ROW_COUNT;
    RETURN v_count;
END
$$;

-- cm: def open_orders
CREATE VIEW shop.open_orders AS
    SELECT o.id, o.customer_id, count(e.kind) AS event_count
    FROM shop.orders o
    LEFT JOIN shop.order_events e ON e.order_id = o.id
    WHERE o.status <> 'cancelled'
    GROUP BY o.id, o.customer_id;
