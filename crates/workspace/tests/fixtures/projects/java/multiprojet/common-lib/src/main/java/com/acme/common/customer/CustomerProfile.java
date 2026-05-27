package com.acme.common.customer;

public record CustomerProfile(String id, String displayName, String segment) {
    public boolean premium() {
        return "premium".equalsIgnoreCase(segment) || "gold".equalsIgnoreCase(segment);
    }

    public String normalizedDisplayName() {
        return this.displayName().trim().toLowerCase();
    }
}
