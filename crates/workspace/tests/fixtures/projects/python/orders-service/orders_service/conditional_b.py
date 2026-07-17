class ConditionalClient:
    def __init__(self, name: str):
        self.name = name

    def label(self) -> str:
        return f"b:{self.name}"
