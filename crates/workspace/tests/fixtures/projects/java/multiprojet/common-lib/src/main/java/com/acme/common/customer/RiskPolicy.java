package com.acme.common.customer;

public class RiskPolicy {
    public boolean isPriority(CustomerProfile profile) {
        return profile.premium() || profile.displayName().trim().startsWith("VIP");
    }

    public int score(CustomerProfile profile) {
        return isPriority(profile) ? 90 : 30;
    }
}
