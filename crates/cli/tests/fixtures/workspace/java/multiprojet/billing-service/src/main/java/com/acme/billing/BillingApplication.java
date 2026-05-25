package com.acme.billing;

import com.acme.common.customer.CustomerDirectory;
import com.acme.common.customer.CustomerProfile;
import com.acme.common.customer.CustomerResolver;
import com.acme.common.money.MoneyFormatter;

public class BillingApplication {
    private final CustomerResolver customerResolver = new CustomerDirectory();
    private final MoneyFormatter moneyFormatter = new MoneyFormatter();

    public String invoiceLine(String customerId, long cents) {
        var profile = customerResolver.resolveCustomer(customerId);
        return moneyFormatter.formatForInvoice(profile, cents);
    }

    public static void main(String[] args) {
        System.out.println(new BillingApplication().invoiceLine("c-100", 1299));
    }
}
