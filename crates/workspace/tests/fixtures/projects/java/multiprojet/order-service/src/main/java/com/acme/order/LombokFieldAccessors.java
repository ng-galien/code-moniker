package com.acme.order;

import lombok.Getter;

public class LombokFieldAccessors {
    @Getter
    private String fieldOnly;

    public String readFieldOnly() {
        return getFieldOnly();
    }
}
