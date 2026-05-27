package com.acme.common.customer;

public class CustomerDirectory implements CustomerResolver {
    @Override
    public CustomerProfile resolveCustomer(String customerId) {
        return new CustomerProfile(customerId, "Customer " + customerId, "premium");
    }

    public String findPreferredSegment(CustomerProfile profile) {
        return profile.premium() ? "high-touch" : "standard";
    }

    public String unusedLegacySegment(String rawSegment) {
        return rawSegment == null ? "unknown" : rawSegment.trim().toLowerCase();
    }
}
