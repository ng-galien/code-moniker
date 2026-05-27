package com.acme.springedge.api;

import com.acme.common.customer.CustomerProfile;
import com.acme.springedge.app.CustomerProfileDto;
import com.acme.springedge.app.SpringCustomerService;
import org.springframework.http.ResponseEntity;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.PathVariable;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RestController;

@RestController
@RequestMapping("/customers")
public class CustomerController {
    private final SpringCustomerService customerService;

    public CustomerController(SpringCustomerService customerService) {
        this.customerService = customerService;
    }

    @GetMapping("/{customerId}")
    public ResponseEntity<CustomerProfileDto> getCustomer(@PathVariable String customerId) {
        CustomerProfile profile = customerService.loadProfile(customerId);
        return ResponseEntity.ok(CustomerProfileDto.from(profile));
    }
}
