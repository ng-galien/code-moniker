package com.acme.nomanifest.outsider;

import com.acme.nomanifest.SharedRecord;

public class OutsiderCaller {
    public String readLabel(SharedRecord record) {
        return record.getLabel();
    }

    public String readDescription(SharedRecord record) {
        return record.describe();
    }
}
