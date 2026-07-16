from orders_service.registry import RepositoryRegistry

service = RepositoryRegistry()
service.register("orders")


def warm_service() -> int:
    service.register("archives")
    return service.count()


def seed_registry(target: RepositoryRegistry) -> int:
    target.register("seeded")
    return target.count()


@service.guard
def audited_entry() -> str:
    return "audited"
