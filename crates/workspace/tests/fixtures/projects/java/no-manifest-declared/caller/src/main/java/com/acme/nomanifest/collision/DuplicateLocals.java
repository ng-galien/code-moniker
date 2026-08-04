package com.acme.nomanifest.collision;

public class DuplicateLocals {
    String read() {
        String value = "caller";
        return value;
    }
}
