package com.acme.override;

public class PlainRecord {
    private final String label;

    public PlainRecord(String label) {
        this.label = label;
    }

    public String label() {
        return label;
    }
}
