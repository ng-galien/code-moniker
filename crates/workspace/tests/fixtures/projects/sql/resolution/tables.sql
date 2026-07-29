CREATE TABLE public.orders (
    id uuid PRIMARY KEY,
    customer_id uuid,
    CONSTRAINT orders_customer_fk
        FOREIGN KEY (customer_id) REFERENCES public.customers(id)
);
