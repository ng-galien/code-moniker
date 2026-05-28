package com.acme.order;

import com.acme.common.customer.CustomerProfile;
import java.util.Map;

public final class OrderContainer {
    public static final class OrderToken {
        private final CustomerProfile profile;

        public OrderToken(CustomerProfile profile) {
            this.profile = profile;
        }

        public CustomerProfile profile() {
            return profile;
        }
    }

    public OrderContainer.OrderToken token(CustomerProfile profile) {
        return new OrderContainer.OrderToken(profile);
    }

    public Map.Entry<String, OrderContainer.OrderToken> entryFor(CustomerProfile profile) {
        return Map.entry("customer", token(profile));
    }

    public String entryRoute(Map.Entry<String, OrderContainer.OrderToken> entry) {
        OrderContainer.OrderToken token = entry.getValue();
        return entry.getKey() + ":" + token.profile().segment();
    }

    public String tokenTypeLabel() {
        return OrderContainer.OrderToken.class.getSimpleName();
    }
}
