package com.acme.nomanifest.caller;

import com.acme.nomanifest.SharedRecord;

public class MainCaller {
    public String readLabel(SharedRecord record) {
        return record.getLabel();
    }
}
