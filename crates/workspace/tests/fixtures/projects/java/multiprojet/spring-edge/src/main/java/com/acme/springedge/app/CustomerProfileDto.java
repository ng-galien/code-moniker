package com.acme.springedge.app;

import com.acme.common.customer.CustomerProfile;

public record CustomerProfileDto(String id, String label, boolean priority) {
    public static CustomerProfileDto from(CustomerProfile profile) {
        String label = profile.normalizedDisplayName();
        return new CustomerProfileDto(profile.id(), label, profile.premium());
    }
}
