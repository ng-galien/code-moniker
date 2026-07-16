class TracingMixin:
    def trace(self, message: str) -> str:
        return f"trace:{message}"


class BaseRepository:
    def __init__(self, dsn: str) -> None:
        self.dsn = dsn

    def open_session(self) -> str:
        return f"session:{self.dsn}"

    def describe_backend(self) -> str:
        return "generic"
