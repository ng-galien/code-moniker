package com.acme.order;

public final class TypedOrderBox<T> {
    private final T value;

    public TypedOrderBox(T value) {
        this.value = value;
    }

    public T value() {
        return value;
    }

    public T castValue() {
        return value;
    }

    public <E> E echo(E value) {
        return value;
    }

    public static <S> S identity(S value) {
        return value;
    }

    public static <O> GenericCreator creator(TypedOrderBox<O> ignored) {
        return new GenericCreator() {
            @Override
            public <I> TypedOrderBox<I> create(I value) {
                return new TypedOrderBox<I>(value);
            }
        };
    }
}
