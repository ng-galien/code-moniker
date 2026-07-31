package com.acme.custom;

public class TestCaller {
    public long read(Clock clock) {
        return clock.now();
    }

    public String readProduction(ProductionOnly production) {
        return production.name();
    }
}
