package com.acme.springedge.app;

import com.acme.common.customer.CustomerProfile;
import com.acme.common.customer.CustomerResolver;
import org.springframework.stereotype.Repository;

@Repository
public class SpringCustomerRepository implements CustomerResolver {
    @Override
    public CustomerProfile resolveCustomer(String customerId) {
        return findById(customerId);
    }

    public CustomerProfile findById(String customerId) {
        return new CustomerProfile(customerId, "VIP " + customerId, "gold");
    }
}
