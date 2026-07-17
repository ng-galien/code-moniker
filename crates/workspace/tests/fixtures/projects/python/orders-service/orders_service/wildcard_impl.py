__all__ = ["ExportedClient"]


class ExportedClient:
    def __init__(self, name: str):
        self.name = name

    @classmethod
    def create(cls, name: str):
        return cls(name)

    def label(self) -> str:
        return self.name
