package com.acme.springedge.config;

import com.acme.common.customer.RiskPolicy;
import org.springframework.context.annotation.Bean;
import org.springframework.context.annotation.Configuration;

@Configuration
public class CustomerConfiguration {
    @Bean
    public RiskPolicy riskPolicy() {
        return new RiskPolicy();
    }
}
