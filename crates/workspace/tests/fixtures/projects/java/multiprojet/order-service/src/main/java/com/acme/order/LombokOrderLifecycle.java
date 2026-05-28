package com.acme.order;

import lombok.extern.slf4j.Slf4j;

@Slf4j
public class LombokOrderLifecycle {
    public String activatePriorityOrder() {
        var state = new LombokOrderState();
        state.setStatus("ACTIVE");
        state.setPriority(true);
        state.getReviewed();
        state.getImmutableCode();
        log.info("Activated priority order with status {}", state.getStatus());
        return state.isPriority() ? state.getStatus() : "PENDING";
    }
}
