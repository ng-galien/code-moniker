from analytics_service.models import Customer, RiskScore


class RiskPolicy:
    def evaluate(self, customer: Customer, features: dict[str, int]) -> RiskScore:
        return RiskScore(customer.customer_id, customer.segment)
