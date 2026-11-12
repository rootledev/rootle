# syntax=docker/dockerfile:1
# BuildKit caches live in the Docker daemon's admitted NVMe data root.
# Always copy executable outputs out of cache mounts in the same RUN.
FROM rust:alpine@sha256:44819b27be4c97b0e4d7203bd2e02c4820f97700211b6a37fb4c246a2de59383 AS builder
RUN apk add --no-cache build-base file git python3 tar \
    && rustup toolchain install 1.100.0 --profile minimal --component clippy,rustfmt \
    && rustup default 1.100.0
ARG TARGETARCH
ENV CARGO_TARGET_DIR=/cache/target
WORKDIR /app
COPY Cargo.toml Cargo.lock README.md LICENSE ./
COPY crates ./crates
COPY .github/scripts ./.github/scripts

FROM builder AS test
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/cache/target,id=rootle-test-${TARGETARCH},sharing=locked \
    find crates -type f -exec touch {} + \
    && cargo fmt --all --check \
    && cargo clippy --locked --workspace --all-targets -- -D warnings \
    && cargo test --locked --workspace \
    && cargo build --locked -p rootle \
    && install -D -m755 /cache/target/debug/rootle /app/target/debug/rootle

FROM builder AS release
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/cache/target,id=rootle-release-${TARGETARCH},sharing=locked \
    find crates -type f -exec touch {} + \
    && cargo build --release --locked -p rootle \
    && install -D -m755 /cache/target/release/rootle /app/target/release/rootle \
    && strip /app/target/release/rootle \
    && readelf -d /app/target/release/rootle > /tmp/dynamic \
    && ! grep -q '(NEEDED)' /tmp/dynamic \
    && file /app/target/release/rootle | grep -qE 'static-pie linked|statically linked'
CMD ["sh", ".github/scripts/package.sh"]

FROM scratch AS ship
COPY --from=release /app/target/release/rootle /rootle
ENTRYPOINT ["/rootle"]

FROM test AS e2e
RUN apk add --no-cache uv
COPY e2e/pyproject.toml e2e/uv.lock ./e2e/
RUN cd e2e && uv sync --locked
COPY e2e ./e2e
COPY install.sh ./install.sh
ENV ROOTLE_E2E_IN_DOCKER=1 CARGO_TARGET_DIR=/app/target
WORKDIR /app/e2e
CMD ["uv", "run", "--locked", "--no-sync", "pytest"]

FROM eclipse-temurin:21-jre@sha256:18bb4a91a3dea3f03ee31d3755af34dfb40f904fc77ab56cdc2f092e32fff9c2 AS model
ARG TLA_TOOLS_SHA256=e1f11b75c4b5bc4478ba0ac5cb6ae28e40a01b0016bc33e0aa97da22dfdbcd7f
ADD --checksum=sha256:${TLA_TOOLS_SHA256} https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar /tla/tla2tools.jar
WORKDIR /work
COPY specs ./specs
RUN sh specs/check.sh

# Qualified releases use a reviewed advisory snapshot, not a moving network head.
FROM builder AS audit
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    cargo install cargo-audit --version 0.23.0 --locked
RUN git init /advisory-db \
    && git -C /advisory-db fetch --depth=1 https://github.com/RustSec/advisory-db.git af7b3ec33460b8e2de8d422e16d628518891a519 \
    && git -C /advisory-db checkout --detach FETCH_HEAD
RUN cargo audit --no-fetch --db /advisory-db
