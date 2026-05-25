package com.acme.order;

public class OrderTestFixture {
    private final OrderApplication application = new OrderApplication();

    public String sampleRoute() {
        return application.routeOrder("c-200");
    }
}
