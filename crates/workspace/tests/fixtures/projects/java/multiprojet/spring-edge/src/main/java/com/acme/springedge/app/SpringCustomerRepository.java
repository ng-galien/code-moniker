package com.acme.springedge.app;

import com.acme.common.customer.CustomerProfile;
import org.springframework.stereotype.Repository;

@Repository
public class SpringCustomerRepository {
    public CustomerProfile findById(String customerId) {
        return new CustomerProfile(customerId, "VIP " + customerId, "gold");
    }
}
