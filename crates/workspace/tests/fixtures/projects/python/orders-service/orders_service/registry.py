class RepositoryRegistry:
    def __init__(self) -> None:
        self.names: list[str] = []

    def register(self, name: str) -> None:
        self.names.append(name)

    def count(self) -> int:
        return len(self.names)


registry = RepositoryRegistry()
