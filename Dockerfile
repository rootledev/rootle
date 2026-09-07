# syntax=docker/dockerfile:1
# Multi-stage musl static build (PLAN.md §13).
# rust:alpine's host triple is the platform's musl native target
# (x86_64 on amd64, aarch64 on arm), so plain `cargo build` already
# produces a static musl binary — the release matrix relies on this
# for native arm builds with zero cross config.

FROM rust:alpine AS builder
RUN apk add --no-cache build-base file \
    && rustup component add clippy rustfmt
WORKDIR /app
COPY Cargo.toml Cargo.lock README.md LICENSE ./
COPY crates ./crates

FROM builder AS test
RUN cargo fmt --all --check \
    && cargo clippy --locked --workspace --all-targets -- -D warnings \
    && cargo test --locked --workspace

# Tree-sitter's C/C++ grammars are compiled into the native musl binary.
# Static PIE may appear dynamic to ldd; ELF dependencies are the real gate.
FROM builder AS release
RUN cargo build --release --locked \
    && strip target/release/rootle \
    && readelf -d target/release/rootle > /tmp/rootle-dynamic \
    && ! grep -q '(NEEDED)' /tmp/rootle-dynamic \
    && file target/release/rootle | grep -qE "static-pie linked|statically linked" \
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

# Bounded protocol model: safety, explicitly fair finite-stream progress,
# and four kept fault classes. Parser/runtime errors never count as a kill.
FROM eclipse-temurin:21-jre AS model
ARG TLA_TOOLS_SHA256=b658b4e504fdf0b721caf7066320f6b6fe5805f4dd2f717d0e47baba4097205e
ADD --checksum=sha256:${TLA_TOOLS_SHA256} https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar /tla/tla2tools.jar
WORKDIR /work
COPY specs ./specs
RUN sh specs/check.sh
