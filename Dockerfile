FROM rust:1.93.0 AS builder

RUN set -ex \
        \
    && apt-get update \
    && apt-get install -y musl-tools lld \
    && rustup target add x86_64-unknown-linux-musl

WORKDIR /opt/app

COPY Cargo.toml /opt/app/Cargo.toml
COPY Cargo.lock /opt/app/Cargo.lock
COPY crates /opt/app/crates

RUN set -ex \
        \
    && export RUSTFLAGS="-C linker=lld" \
    && cargo build --release --target=x86_64-unknown-linux-musl


FROM alpine:3.23.0 AS runtime

RUN set -ex \
        \
    && apk add --update --no-cache tini curl

WORKDIR /opt/app

COPY --from=builder /opt/app/target/x86_64-unknown-linux-musl/release/ddns /opt/app/ddns
COPY ddns.example.toml /opt/app/ddns.toml

ENTRYPOINT ["/sbin/tini", "-s", "--"]
CMD ["./ddns"]
