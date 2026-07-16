from orders_service.registry import RepositoryRegistry

service = RepositoryRegistry()
service.register("orders")


def warm_service() -> int:
    service.register("archives")
    return service.count()
