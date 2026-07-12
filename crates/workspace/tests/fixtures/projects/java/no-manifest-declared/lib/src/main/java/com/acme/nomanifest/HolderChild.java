package com.acme.nomanifest;

public class HolderChild extends BaseHolder {
    public String useRecord() {
        return record.describe();
    }

    public String useHelper() {
        return helper.help();
    }
}
