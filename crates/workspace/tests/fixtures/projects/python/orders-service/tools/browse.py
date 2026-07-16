from orders_service import catalog
from orders_service.catalog import entries


def browse_default() -> str:
    entry = catalog.CatalogEntry("default")
    return entry.entry_key()


def browse_entry(sku: str) -> str:
    entry = entries.CatalogEntry(sku)
    return entry.entry_key()


def browse_all() -> list[str]:
    made = entries.make_entry("made")
    default = catalog.default_entry()
    return [made.entry_key(), default.entry_key()]


def browse_fallback() -> str:
    entry = entries.make_default_entry()
    return entry.entry_key()
