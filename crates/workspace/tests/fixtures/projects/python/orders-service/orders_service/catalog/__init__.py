from orders_service.catalog.entries import CatalogEntry


def default_entry() -> CatalogEntry:
    return CatalogEntry("default")


__all__ = ["CatalogEntry", "default_entry"]
