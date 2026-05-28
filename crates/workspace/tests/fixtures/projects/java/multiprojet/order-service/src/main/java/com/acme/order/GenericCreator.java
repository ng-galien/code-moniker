package com.acme.order;

public interface GenericCreator {
    <U> TypedOrderBox<U> create(U value);
}
