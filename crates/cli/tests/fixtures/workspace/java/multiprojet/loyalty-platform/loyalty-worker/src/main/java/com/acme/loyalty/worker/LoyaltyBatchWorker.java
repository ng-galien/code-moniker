package com.acme.loyalty.worker;

import com.acme.common.customer.CustomerDirectory;
import com.acme.common.customer.CustomerProfile;
import com.acme.common.customer.RiskPolicy;

public class LoyaltyBatchWorker {
    private final CustomerDirectory customerDirectory = new CustomerDirectory();
    private final RiskPolicy riskPolicy = new RiskPolicy();

    public int computePriorityScore(String customerId) {
        var profile = customerDirectory.resolveCustomer(customerId);
        return riskPolicy.score(profile);
    }

    public String routeBatch(String customerId) {
        return computePriorityScore(customerId) >= 80 ? "priority-batch" : "standard-batch";
    }

    public static void main(String[] args) {
        System.out.println(new LoyaltyBatchWorker().routeBatch("c-500"));
    }
}
