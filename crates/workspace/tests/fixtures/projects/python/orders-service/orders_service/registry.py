class RepositoryRegistry:
    def __init__(self) -> None:
        self.names: list[str] = []

    def register(self, name: str) -> None:
        self.names.append(name)

    def count(self) -> int:
        return len(self.names)

    def guard(self, func):
        self.names.append(func.__name__)
        return func


registry = RepositoryRegistry()
