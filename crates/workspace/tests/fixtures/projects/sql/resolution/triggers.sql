CREATE TRIGGER audit_orders
AFTER INSERT OR UPDATE ON public.orders
FOR EACH ROW EXECUTE FUNCTION public.audit_order('orders');

CREATE TRIGGER route_open_orders
INSTEAD OF UPDATE ON public.open_orders
FOR EACH ROW EXECUTE FUNCTION public.route_open_order();
