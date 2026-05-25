package com.acme.order;

import com.acme.common.customer.CustomerDirectory;
import com.acme.common.customer.CustomerProfile;
import com.acme.common.customer.RiskPolicy;

public class OrderArchitectureTest {
    private final OrderApplication application = new OrderApplication();
    private final CustomerDirectory customerDirectory = new CustomerDirectory();
    private final RiskPolicy riskPolicy = new RiskPolicy();

    public String expectedRouteForPremiumCustomer() {
        var profile = customerDirectory.resolveCustomer("c-200");
        return riskPolicy.isPriority(profile) ? "priority-lane" : "standard-lane";
    }

    public String actualRouteForPremiumCustomer() {
        return application.routeOrder("c-200");
    }
}
