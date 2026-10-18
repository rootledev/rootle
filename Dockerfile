# syntax=docker/dockerfile:1@sha256:7b32114e84ca21aeb2d6f871a0b62b3a0762193f75d9afd048ee4cbe889f3148
# Trusted-code, nonpublishing Linux/WSL x86_64 pilot only.
# The pinned amd64 Rust image uses x86_64-unknown-linux-musl natively.
# No ARM, macOS, or native Windows build path is supported.

FROM rust:alpine@sha256:3ffeca71d0e4fc30f5537f76b7243e87ac99726b6d3d66591dfc5e497078b9fc AS builder
ARG CARGO_BUILD_JOBS=4
ENV CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS}
# Complete APK closure is byte-pinned; no repository index participates in
# installation. CDN retention is still required: missing/replaced bytes fail.
COPY docker-apk.lock /opt/rootle/docker-apk.lock
RUN set -eu; \
    mkdir -p /tmp/build-apks; \
    while read -r scope checksum url; do \
        [ "$scope" = build ] || continue; \
        archive="/tmp/build-apks/${url##*/}"; \
        wget -O "$archive" "$url"; \
        printf '%s  %s\n' "$checksum" "$archive" | sha256sum -c -; \
    done < /opt/rootle/docker-apk.lock; \
    apk --no-network --repositories-file /dev/null add /tmp/build-apks/*.apk; \
    rm -rf /tmp/build-apks; \
    rustup component add --toolchain 1.98.0 clippy rustfmt
WORKDIR /app
COPY Cargo.toml Cargo.lock README.md LICENSE install.sh ./
COPY crates ./crates
COPY .ci ./.ci

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

# Audit tool source and its bundled dependency lock are immutable.
# Runtime advisory data is intentionally separate from the tool identity.
FROM builder AS audit
ADD --checksum=sha256:700c2b240f7fd330c24b675fe429f73a5b676531fcc6300400b2b67f155ba12a https://static.crates.io/crates/cargo-audit/cargo-audit-0.22.2.crate /tmp/cargo-audit.crate
RUN tar -xzf /tmp/cargo-audit.crate -C /tmp \
    && cargo install --locked --path /tmp/cargo-audit-0.22.2 \
    && rm -rf /tmp/cargo-audit.crate /tmp/cargo-audit-0.22.2
CMD ["cargo", "audit"]

# e2e PTY suite (plans/0002-v0.2 §6, productionize). FROM test reuses
# the gate's compiled target/ — no second artifact tree, and the gate
# must have passed for this stage to build at all.
FROM test AS e2e
# python3+uv for the harness; git for the clone-wizard e2e (fs provider
# repos are real local git remotes).
RUN set -eu; \
    mkdir -p /tmp/e2e-apks; \
    while read -r scope checksum url; do \
        [ "$scope" = e2e ] || continue; \
        archive="/tmp/e2e-apks/${url##*/}"; \
        wget -O "$archive" "$url"; \
        printf '%s  %s\n' "$checksum" "$archive" | sha256sum -c -; \
    done < /opt/rootle/docker-apk.lock; \
    apk --no-network --repositories-file /dev/null add /tmp/e2e-apks/*.apk; \
    rm -rf /tmp/e2e-apks
# Never let uv download a different Python interpreter.
ENV UV_PYTHON_DOWNLOADS=never UV_PYTHON=/usr/bin/python3
COPY e2e/pyproject.toml e2e/uv.lock ./e2e/
RUN cargo build --locked && cd e2e && uv sync --locked
COPY e2e ./e2e
# Binary already compiled by the gate stage; harness must not rebuild.
ENV ROOTLE_E2E_IN_DOCKER=1
WORKDIR /app/e2e
CMD ["uv", "run", "--locked", "--no-sync", "pytest"]

# Bounded protocol model: safety, explicitly fair finite-stream progress,
# and four kept fault classes. Parser/runtime errors never count as a kill.
FROM eclipse-temurin:21-jre@sha256:e6354536ca587af353f4927095003b3a5fc29fe1a1380fa1e55caec12815d188 AS model
# Upstream asset 644223998: release API digest matched downloaded bytes.
ARG TLA_TOOLS_SHA256=15d103b7e5617780bcc904c0af12f1ad242e2b345c0301136c5be4588f17edac
ADD --checksum=sha256:${TLA_TOOLS_SHA256} https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar /tla/tla2tools.jar
WORKDIR /work
COPY specs ./specs
RUN sh specs/check.sh
