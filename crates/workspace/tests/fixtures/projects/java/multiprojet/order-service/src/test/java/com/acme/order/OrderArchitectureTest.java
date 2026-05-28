package com.acme.order;

import static com.google.common.truth.Truth.assertThat;

import com.acme.common.customer.CustomerDirectory;
import com.acme.common.customer.CustomerProfile;
import com.acme.common.customer.RiskPolicy;
import org.junit.Test;

@Deprecated
@SuppressWarnings("deprecation")
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

    @Test
    public void routesPremiumCustomerThroughPriorityLane() {
        assertThat(actualRouteForPremiumCustomer()).isEqualTo(expectedRouteForPremiumCustomer());
        assertThat(riskPolicy.isPriority(customerDirectory.resolveCustomer("c-200"))).isTrue();
    }
}
