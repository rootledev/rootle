# syntax=docker/dockerfile:1
# Multi-stage musl static build (PLAN.md §13).
# rust:alpine's host triple is the platform's musl native target
# (x86_64 on amd64, aarch64 on arm), so plain `cargo build` already
# produces a static musl binary — the release matrix relies on this
# for native arm builds with zero cross config.

FROM rust:alpine AS builder
RUN apk add --no-cache musl-dev \
    && rustup component add clippy rustfmt
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests
COPY examples ./examples

FROM builder AS test
RUN cargo fmt --check \
    && cargo clippy --locked --all-targets -- -D warnings \
    && cargo test --locked

# Stripped static release binary.
FROM builder AS release
RUN cargo build --release --locked \
    && strip target/release/rootle \
    && ldd target/release/rootle 2>&1 | grep -q "Not a valid dynamic program\|not a dynamic executable" \
    && echo "static: ok"

# Shipping image: just the binary.
FROM scratch AS ship
COPY --from=release /app/target/release/rootle /rootle
ENTRYPOINT ["/rootle"]

# e2e PTY suite (plans/0002-v0.2 §6, productionize). FROM test reuses
# the gate's compiled target/ — no second artifact tree, and the gate
# must have passed for this stage to build at all.
FROM test AS e2e
# python3+uv for the harness; git for the clone-wizard e2e (fs provider
# repos are real local git remotes).
RUN apk add --no-cache python3 uv git
COPY e2e/pyproject.toml e2e/uv.lock ./e2e/
RUN cargo build --locked && cd e2e && uv sync --locked
COPY e2e ./e2e
# Binary already compiled by the gate stage; harness must not rebuild.
ENV ROOTLE_E2E_IN_DOCKER=1
WORKDIR /app/e2e
CMD ["uv", "run", "--locked", "--no-sync", "pytest"]
