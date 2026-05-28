package com.acme.order;

import lombok.Data;

@Data
public class LombokOrderState {
    private String status;
    private boolean priority;
    private Boolean reviewed;
    private final String immutableCode = "LOCKED";
}
