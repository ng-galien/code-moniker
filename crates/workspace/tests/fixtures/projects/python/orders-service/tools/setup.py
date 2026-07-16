from orders_service.registry import registry


def register_defaults() -> int:
    registry.register("orders")
    return registry.count()
