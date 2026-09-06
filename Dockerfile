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
COPY crates ./crates
COPY src ./src
COPY tests ./tests
COPY examples ./examples

FROM builder AS test
RUN cargo fmt --all --check \
    && cargo clippy --locked --workspace --all-targets -- -D warnings \
    && cargo test --locked --workspace

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

# The provider protocol model check (plans/0027): TLC over
# specs/ProviderProtocol.tla — and, unlike strop's gate, over the kept
# mutant too, so the invariants cannot rot: the base spec must check
# clean AND the mutant must FAIL with CorrelationSafety. The image
# existing IS the gate — a spec regression fails the build.
FROM eclipse-temurin:21-jre AS model
ARG TLA_TOOLS_SHA256=b658b4e504fdf0b721caf7066320f6b6fe5805f4dd2f717d0e47baba4097205e
ADD --checksum=sha256:${TLA_TOOLS_SHA256} https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar /tla/tla2tools.jar
WORKDIR /work
COPY specs ./specs
# Base: bounded-exhaustive check must come back clean.
RUN java -jar /tla/tla2tools.jar -cleanup -config specs/cfg/provider-protocol.cfg specs/ProviderProtocol.tla > /tmp/base.log 2>&1 \
    && grep -q "Model checking completed. No error" /tmp/base.log \
    || { echo "TLC failed for ProviderProtocol (base)"; cat /tmp/base.log; exit 1; }; \
    echo "ProviderProtocol: clean"
# Mutant: must FAIL, and by name — a zero exit means the invariant lost
# its teeth; a nonzero exit without CorrelationSafety means the fault
# moved and the pairing must be re-examined.
RUN java -jar /tla/tla2tools.jar -cleanup -config specs/cfg/provider-protocol-mutant.cfg specs/ProviderProtocol_Mutant.tla > /tmp/mutant.log 2>&1; \
    code=$?; \
    if [ "$code" -eq 0 ]; then \
        echo "KEPT MUTANT PASSED TLC — CorrelationSafety lost its teeth"; \
        cat /tmp/mutant.log; exit 1; \
    fi; \
    grep -q "CorrelationSafety" /tmp/mutant.log \
    || { echo "mutant failed (exit $code) but not via CorrelationSafety"; cat /tmp/mutant.log; exit 1; }; \
    echo "ProviderProtocol_Mutant: killed by CorrelationSafety"
