package com.acme.nomanifest.caller;

import com.acme.nomanifest.ChannelFactory;
import com.acme.nomanifest.RecordFactory;

public class ChainCaller {
    public String chainThrough() {
        return factory().make().describe();
    }

    private RecordFactory factory() {
        return new RecordFactory();
    }

    public String ackViaChain(ChannelFactory channels) {
        return channels.open().ack();
    }
}
