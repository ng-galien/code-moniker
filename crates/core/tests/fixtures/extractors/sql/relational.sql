CREATE SCHEMA IF NOT EXISTS Sales;

CREATE TABLE Sales."Orders" (
    "ID"        uuid CONSTRAINT "Orders_PK" PRIMARY KEY,
    customer_id uuid NOT NULL,
    CONSTRAINT orders_customer_fk
        FOREIGN KEY (customer_id) REFERENCES crm.customers(id)
);

CREATE VIEW Sales.open_orders AS
WITH recent AS (
    SELECT * FROM Sales."Orders"
)
SELECT r."ID"
FROM recent r
JOIN crm.customers c ON c.id = r.customer_id;

CREATE FUNCTION Sales.customer_orders()
RETURNS SETOF Sales."Orders"
LANGUAGE sql
AS $$
    SELECT * FROM Sales."Orders"
$$;

INSERT INTO audit.events
SELECT * FROM Sales."Orders";

CREATE TABLE Sales.orders_copy AS
SELECT * FROM Sales."Orders";

CREATE TRIGGER audit_orders
AFTER INSERT OR UPDATE ON Sales."Orders"
FOR EACH ROW EXECUTE FUNCTION audit.log_order('orders');

CREATE PROCEDURE Sales.dynamic_refresh(target_table text)
LANGUAGE plpgsql
AS $$
BEGIN
    EXECUTE 'INSERT INTO audit.' || quote_ident(target_table);
END;
$$;
