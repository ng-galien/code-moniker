class Customer:
    def __init__(self, customer_id: str, segment: str) -> None:
        self.customer_id = customer_id
        self.segment = segment


class RiskScore:
    def __init__(self, customer_id: str, label: str) -> None:
        self.customer_id = customer_id
        self.label = label
