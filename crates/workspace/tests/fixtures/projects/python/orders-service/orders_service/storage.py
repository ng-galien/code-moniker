class TracingMixin:
    def trace(self, message: str) -> str:
        return f"trace:{message}"


def workspace_only_flag() -> str:
    return "storage"


class BaseRepository:
    def __init__(self, dsn: str) -> None:
        self.dsn = dsn

    def open_session(self) -> str:
        return f"session:{self.dsn}"

    @property
    def dsn_scheme(self) -> str:
        return self.dsn.split(":", 1)[0]

    def describe_backend(self) -> str:
        return "generic"
