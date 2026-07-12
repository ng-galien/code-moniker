package com.acme.order;

public class LombokOrderBuilderUsage {
    public LombokBuildableOrder assemble() {
        return LombokBuildableOrder.builder()
                .reference("PO-1")
                .status("DRAFT")
                .build();
    }
}
