package com.acme.nomanifest;

public class ClockUser {
    public long read(Clock clock) {
        return clock.now();
    }
}
