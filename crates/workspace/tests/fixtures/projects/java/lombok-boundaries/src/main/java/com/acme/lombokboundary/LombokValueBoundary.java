package com.acme.lombokboundary;

import lombok.Value;

@Value
public class LombokValueBoundary {
    String code = "LOCKED";

    public void exercise() {
        this.withCode("OPEN");
    }
}
