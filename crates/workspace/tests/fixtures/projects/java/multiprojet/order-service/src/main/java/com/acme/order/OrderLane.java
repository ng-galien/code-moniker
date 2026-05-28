package com.acme.order;

public enum OrderLane {
    PRIORITY("priority-lane"),
    STANDARD("standard-lane"),
    REVIEW("review-lane");

    private final String route;

    OrderLane(String route) {
        this.route = route;
    }

    public String route() {
        return route;
    }

    public boolean requiresReview() {
        return this == REVIEW;
    }
}
