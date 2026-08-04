package com.acme.nomanifest.caller;

import com.acme.nomanifest.exports.*;
import static com.acme.nomanifest.exports.Tools.*;

public class WildcardCaller {
    Widget create() {
        return new Widget();
    }

    String label() {
        return decorate();
    }

    String invalidStaticImport() {
        return instanceOnly();
    }

    String localRead() {
        String value = "local";
        return value;
    }
}
