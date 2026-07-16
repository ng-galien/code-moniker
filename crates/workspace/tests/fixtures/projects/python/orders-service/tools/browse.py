from orders_service import catalog
from orders_service.catalog import entries


def browse_default() -> str:
    entry = catalog.CatalogEntry("default")
    return entry.entry_key()


def browse_entry(sku: str) -> str:
    entry = entries.CatalogEntry(sku)
    return entry.entry_key()
