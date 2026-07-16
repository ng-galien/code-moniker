from orders_service.storage import BaseRepository, TracingMixin

RETRYABLE_ERRORS = (KeyError, TimeoutError)


class OrderRepository(BaseRepository):
    def load_orders(self) -> list[str]:
        session = self.open_session()
        return [session]

    def backend_label(self) -> str:
        return self.describe_backend()


class ArchivedOrderRepository(OrderRepository):
    def load_archived(self) -> list[str]:
        rows = self.load_orders()
        handle = self.open_session()
        return rows + [handle]


class AuditedOrderRepository(TracingMixin, OrderRepository):
    def load_audited(self) -> list[str]:
        self.trace("load")
        session = self.open_session()
        return [session]
