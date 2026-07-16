class CatalogEntry:
    def __init__(self, sku: str) -> None:
        self.sku = sku

    def entry_key(self) -> str:
        return f"entry:{self.sku}"
