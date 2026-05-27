package com.acme.billing;

public class BillingTestFixture {
    private final BillingApplication application = new BillingApplication();

    public String sampleInvoiceLine() {
        return application.invoiceLine("c-100", 1299);
    }
}
