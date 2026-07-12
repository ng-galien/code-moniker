package com.acme.nomanifest;

import com.thirdparty.util.Helper;

public class ThirdPartyUser {
    public String describeHelper() {
        Helper helper = new Helper();
        return helper.help();
    }

    public String describeMissing() {
        return MissingRecord.create().getLabel();
    }
}
