from orders_service.catalog import CatalogEntry


class PricedEntry(CatalogEntry):
    def price_key(self) -> str:
        return f"priced:{self.entry_key()}"


def first_entry() -> CatalogEntry:
    return CatalogEntry("default")


def first_entry_key_direct() -> str:
    return first_entry().entry_key()


def first_entry_key_assigned() -> str:
    entry = first_entry()
    return entry.entry_key()


def first_entry_key_normalized() -> str:
    return first_entry().entry_key().strip()


class LocalNormalizer:
    def normalize(self) -> str:
        return "local"


def normalize_unknown(value):
    return value.normalize()


def read_unknown():
    return workspace_only_flag


def local_callback():
    return "local"


def read_local_callback():
    return local_callback


def call_missing():
    return missing_callable()
