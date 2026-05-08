# syntax=docker/dockerfile:1

# ---------- builder ----------
FROM rust:1.95-bookworm AS builder

ARG PG_MAJOR=17
ARG PGRX_VERSION=0.18.0

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        ca-certificates curl gnupg lsb-release \
        build-essential pkg-config libssl-dev libclang-dev \
    && install -d /usr/share/postgresql-common/pgdg \
    && curl -fsSL -o /usr/share/postgresql-common/pgdg/apt.postgresql.org.asc \
        https://www.postgresql.org/media/keys/ACCC4CF8.asc \
    && echo "deb [signed-by=/usr/share/postgresql-common/pgdg/apt.postgresql.org.asc] http://apt.postgresql.org/pub/repos/apt $(lsb_release -cs)-pgdg main" \
        > /etc/apt/sources.list.d/pgdg.list \
    && apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        postgresql-${PG_MAJOR} postgresql-server-dev-${PG_MAJOR} \
    && rm -rf /var/lib/apt/lists/*

RUN cargo install --locked cargo-pgrx --version ${PGRX_VERSION}
RUN cargo pgrx init --pg${PG_MAJOR}=/usr/lib/postgresql/${PG_MAJOR}/bin/pg_config

WORKDIR /src
COPY . .

RUN cargo pgrx package \
        --features pg${PG_MAJOR} --no-default-features \
        --pg-config /usr/lib/postgresql/${PG_MAJOR}/bin/pg_config

# ---------- runtime ----------
FROM postgres:17

ARG PG_MAJOR=17

COPY --from=builder /src/target/release/pg_code_moniker-pg${PG_MAJOR}/usr/lib/postgresql/${PG_MAJOR}/lib/pg_code_moniker.so \
    /usr/lib/postgresql/${PG_MAJOR}/lib/
COPY --from=builder /src/target/release/pg_code_moniker-pg${PG_MAJOR}/usr/share/postgresql/${PG_MAJOR}/extension/ \
    /usr/share/postgresql/${PG_MAJOR}/extension/
