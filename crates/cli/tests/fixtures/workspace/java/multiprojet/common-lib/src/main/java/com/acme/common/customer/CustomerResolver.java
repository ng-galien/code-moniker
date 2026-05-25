package com.acme.common.customer;

public interface CustomerResolver {
    CustomerProfile resolveCustomer(String customerId);
}
