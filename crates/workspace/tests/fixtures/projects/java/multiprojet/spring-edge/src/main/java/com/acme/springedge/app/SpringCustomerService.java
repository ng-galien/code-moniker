package com.acme.springedge.app;

import com.acme.common.customer.CustomerProfile;
import com.acme.common.customer.RiskPolicy;
import org.springframework.stereotype.Service;

@Service
public class SpringCustomerService {
    private final SpringCustomerRepository repository;
    private final RiskPolicy riskPolicy;

    public SpringCustomerService(SpringCustomerRepository repository, RiskPolicy riskPolicy) {
        this.repository = repository;
        this.riskPolicy = riskPolicy;
    }

    public CustomerProfile loadProfile(String customerId) {
        CustomerProfile profile = repository.findById(customerId);
        if (riskPolicy.isPriority(profile)) {
            return profile;
        }
        return new CustomerProfile(profile.id(), profile.displayName(), "standard");
    }
}
