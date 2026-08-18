# syntax=docker/dockerfile:1
# Multi-stage musl static build (PLAN.md §13).
# rust:alpine's host triple IS x86_64-unknown-linux-musl, so plain
# `cargo build` already produces a static musl binary.

FROM rust:alpine AS builder
RUN apk add --no-cache musl-dev \
    && rustup component add clippy rustfmt
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests

# Quality gate stage: fmt + clippy + tests. Built by the `test` service.
FROM builder AS test
RUN cargo fmt --check \
    && cargo clippy --locked --all-targets -- -D warnings \
    && cargo test --locked

# Stripped static release binary.
FROM builder AS release
RUN cargo build --release --locked \
    && strip target/release/ghx \
    && ldd target/release/ghx 2>&1 | grep -q "Not a valid dynamic program\|not a dynamic executable" \
    && echo "static: ok"

# Shipping image: just the binary.
FROM scratch AS ship
COPY --from=release /app/target/release/ghx /ghx
ENTRYPOINT ["/ghx"]
