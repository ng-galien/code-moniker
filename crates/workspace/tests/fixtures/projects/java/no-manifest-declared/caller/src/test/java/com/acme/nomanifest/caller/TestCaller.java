package com.acme.nomanifest.caller;

import com.acme.nomanifest.SharedRecord;

public class TestCaller {
    public String readLabel(SharedRecord record) {
        return record.getLabel();
    }

    public String readDescription(SharedRecord record) {
        return record.describe();
    }
}
