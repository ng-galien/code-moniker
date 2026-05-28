package com.acme.lombokboundary;

import lombok.Data;

@Data
public class LombokDataBoundary {
    private final String code = "LOCKED";
    private Boolean reviewed;

    public void exercise() {
        this.setCode("OPEN");
        this.isReviewed();
    }
}
