package com.acme.loyalty.api;

import com.acme.common.customer.CustomerDirectory;
import com.acme.common.customer.CustomerProfile;
import com.acme.common.customer.CustomerResolver;
import com.acme.common.customer.RiskPolicy;
import com.acme.common.money.MoneyFormatter;

public class LoyaltyApplication {
    private final CustomerResolver customerResolver = new CustomerDirectory();
    private final RiskPolicy riskPolicy = new RiskPolicy();
    private final MoneyFormatter moneyFormatter = new MoneyFormatter();

    public String previewReward(String customerId, long cents) {
        var profile = (CustomerProfile) customerResolver.resolveCustomer(customerId);
        var amount = moneyFormatter.formatCents(cents, "EUR");
        return riskPolicy.isPriority(profile)
            ? profile.normalizedDisplayName() + " earns priority reward " + amount
            : profile.normalizedDisplayName() + " earns standard reward " + amount;
    }

    public static void main(String[] args) {
        System.out.println(new LoyaltyApplication().previewReward("c-400", 2500));
    }
}
