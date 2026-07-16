class CatalogEntry:
    def __init__(self, sku: str) -> None:
        self.sku = sku

    def entry_key(self) -> str:
        return f"entry:{self.sku}"


def make_entry(sku: str) -> CatalogEntry:
    return CatalogEntry(sku)


def make_default_entry(sku: str = "default") -> CatalogEntry:
    return CatalogEntry(sku)
