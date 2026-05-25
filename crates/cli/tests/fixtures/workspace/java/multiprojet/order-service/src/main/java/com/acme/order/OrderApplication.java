package com.acme.order;

import com.acme.common.customer.CustomerDirectory;
import com.acme.common.customer.CustomerProfile;
import com.acme.common.customer.CustomerResolver;
import com.acme.common.customer.RiskPolicy;

public class OrderApplication {
    private final CustomerResolver customerResolver = new CustomerDirectory();
    private final RiskPolicy riskPolicy = new RiskPolicy();

    public String routeOrder(String customerId) {
        var profile = customerResolver.resolveCustomer(customerId);
        return riskPolicy.isPriority(profile) ? "priority-lane" : "standard-lane";
    }

    public static void main(String[] args) {
        System.out.println(new OrderApplication().routeOrder("c-200"));
    }
}
