package com.acme.common.money;

import com.acme.common.customer.CustomerProfile;

public class MoneyFormatter {
    public String formatForInvoice(CustomerProfile profile, long cents) {
        return "[invoice] " + profile.displayName() + " owes " + formatCents(cents, "EUR");
    }

    public String formatCents(long cents, String currency) {
        var euros = cents / 100;
        var remainder = Math.abs(cents % 100);
        return currency + " " + euros + "." + String.format("%02d", remainder);
    }

    public String unusedLegacyFormat(long cents) {
        return Long.toString(cents);
    }
}
