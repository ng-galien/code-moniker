from orders_service.catalog import CatalogEntry


class PricedEntry(CatalogEntry):
    def price_key(self) -> str:
        return f"priced:{self.entry_key()}"


def first_entry() -> CatalogEntry:
    return CatalogEntry("default")
