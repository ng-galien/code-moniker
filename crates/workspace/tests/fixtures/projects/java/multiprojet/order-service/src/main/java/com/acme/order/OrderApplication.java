package com.acme.order;

import com.acme.common.customer.CustomerDirectory;
import com.acme.common.customer.CustomerProfile;
import com.acme.common.customer.CustomerResolver;
import com.acme.common.customer.RiskPolicy;

public class OrderApplication {
    private final CustomerResolver customerResolver = new CustomerDirectory();
    private final RiskPolicy riskPolicy = new RiskPolicy();

    public String routeOrder(String customerId) {
        var profile = customerResolver.resolveCustomer(customerId);
        var boxedProfile = new TypedOrderBox<CustomerProfile>(profile);
        Object rawBox = boxedProfile;
        var castProfile = ((TypedOrderBox<CustomerProfile>) rawBox).castValue();
        var routedProfile = boxedProfile.echo(castProfile);
        var stableProfile = TypedOrderBox.identity(routedProfile);
        GenericCreator creator = TypedOrderBox.creator(boxedProfile);
        TypedOrderBox<CustomerProfile> anonymousBox = creator.create(stableProfile);
        var anonymousProfile = anonymousBox.value();
        OrderLane lane = this.selectLane(anonymousProfile);
        return lane.route();
    }

    private OrderLane selectLane(CustomerProfile profile) {
        OrderLane scoredLane = switch (riskPolicy.score(profile)) {
            case 90 -> OrderLane.PRIORITY;
            case 30 -> OrderLane.STANDARD;
            default -> OrderLane.REVIEW;
        };

        switch (profile.segment()) {
            case "premium":
            case "gold":
                return scoredLane;
            default:
                return scoredLane.requiresReview() ? OrderLane.REVIEW : OrderLane.STANDARD;
        }
    }

    public static void main(String[] args) {
        System.out.println(new OrderApplication().routeOrder("c-200"));
    }
}
