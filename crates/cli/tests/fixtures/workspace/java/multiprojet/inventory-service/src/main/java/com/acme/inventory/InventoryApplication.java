package com.acme.inventory;

import com.acme.common.customer.CustomerProfile;
import com.acme.common.customer.RiskPolicy;

public class InventoryApplication {
    private final RiskPolicy riskPolicy = new RiskPolicy();

    public String reservationBucket(String customerId, String displayName, String segment) {
        var profile = new CustomerProfile(customerId, displayName, segment);
        return riskPolicy.isPriority(profile) ? "reserved-premium" : "reserved-standard";
    }

    public static void main(String[] args) {
        System.out.println(new InventoryApplication().reservationBucket("c-300", "VIP Customer", "standard"));
    }
}
