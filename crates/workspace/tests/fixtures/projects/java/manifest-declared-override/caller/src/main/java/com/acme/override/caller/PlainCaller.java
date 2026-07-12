package com.acme.override.caller;

import com.acme.override.PlainRecord;

public class PlainCaller {
    public String readLabel(PlainRecord record) {
        return record.label();
    }
}
