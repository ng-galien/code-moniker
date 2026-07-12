package com.acme.nomanifest.caller;

import com.acme.nomanifest.Channel;

public class ChannelCaller {
    public String ackThrough(Channel channel) {
        return channel.ack();
    }
}
