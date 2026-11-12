# syntax=docker/dockerfile:1
# Rootle request kernel: the `proofs` toolchain image. Pinned Lean 4 + lake + mathlib,
# Charon built from source at the exact commit the pinned Aeneas release
# expects, and that Aeneas release. BuildKit cache mounts keep rebuilds
# cheap; nothing is installed on the host. linux/amd64 only: Aeneas ships
# x86_64 binaries and Charon's nightly rustc lives only here — the shipped
# rootle binary still builds on stable musl with rustls (AGENTS.md).
#
#   docker compose run --build --rm proofs            # the gate
#   sh proofs/refresh.sh                              # regenerate proofs/Extracted/
#
# Every pin below moves deliberately and together:
#   AENEAS_RELEASE  → its `charon-pin` names CHARON_COMMIT, its
#                     backends/lean/lean-toolchain names proofs/lean-toolchain,
#                     its lakefile names the mathlib rev in lake-manifest.json
#   CHARON_COMMIT   → charon/rust-toolchain names CHARON_RUST_TOOLCHAIN

FROM ubuntu:24.04@sha256:224a1869083a311ef3f13648a154ba79832fbef6364d31493642ca03082da254 AS tools
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update -qq && apt-get install -y -qq --no-install-recommends \
    ca-certificates curl git build-essential pkg-config python3 unzip xz-utils zstd \
    && rm -rf /var/lib/apt/lists/*

# Charon's nightly rustc (charon/rust-toolchain at CHARON_COMMIT). rustc-dev
# and llvm-tools link the rustc driver; rust-src and miri build Charon's
# full-MIR sysroot on first use.
ARG CHARON_RUST_TOOLCHAIN=nightly-2026-09-17
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path \
      --default-toolchain ${CHARON_RUST_TOOLCHAIN} \
      --component rustc-dev --component llvm-tools --component rust-src --component miri \
    && rustc --version

# Charon from source at the commit Aeneas's `charon-pin` names. No Charon
# release was cut at that commit (the nightlies before and after it carry
# LLBC versions 0.1.313 and 0.1.314), so no binary pin exists.
ARG CHARON_COMMIT=8cae8fd1905946de61147a65ecdd7122cc6b491b
RUN --mount=type=cache,target=/opt/cargo/registry,sharing=locked \
    --mount=type=cache,target=/opt/cargo/git,sharing=locked \
    --mount=type=cache,target=/build/charon,sharing=locked \
    git init -q /src/charon && cd /src/charon \
    && git fetch -q --depth 1 https://github.com/AeneasVerif/charon ${CHARON_COMMIT} \
    && git checkout -q FETCH_HEAD \
    && git submodule update -q --init --depth 1 \
    && cd charon && CARGO_TARGET_DIR=/build/charon cargo build --release --locked \
    && mkdir -p /opt/charon/bin \
    && install -m755 /build/charon/release/charon /build/charon/release/charon-driver /opt/charon/bin/ \
    && cp rust-toolchain /opt/charon/bin/rust-toolchain \
    && cd / && rm -rf /src/charon
ENV PATH=/opt/charon/bin:$PATH CHARON_CACHE_DIR=/opt/charon-cache

# Lean via a pinned elan release; the toolchain itself is proofs/lean-toolchain.
ARG ELAN_VERSION=v4.2.4
ARG ELAN_SHA256=42b94d4244e8353142c456ec0e4ca6528fd898a6c604d4059f494e706e431f63
ADD --checksum=sha256:${ELAN_SHA256} https://github.com/leanprover/elan/releases/download/${ELAN_VERSION}/elan-x86_64-unknown-linux-gnu.tar.gz /tmp/elan.tar.gz
ENV ELAN_HOME=/opt/elan PATH=/opt/elan/bin:$PATH
COPY proofs/lean-toolchain /tmp/lean-toolchain
RUN mkdir -p /tmp/elan && tar -xzf /tmp/elan.tar.gz -C /tmp/elan \
    && /tmp/elan/elan-init -y --no-modify-path --default-toolchain "$(cat /tmp/lean-toolchain)" \
    && rm -rf /tmp/elan /tmp/elan.tar.gz /tmp/lean-toolchain \
    && lean --version && lake --version

# The Aeneas release: the translator binary plus its Lean backend (sources
# and prebuilt .lake/build). lakefile.toml requires it by this path.
ARG AENEAS_RELEASE=nightly-2026.11.07-c5eb4a2
ARG AENEAS_SHA256=14fbaed4ce8624cf3d668f887d6ebb237a59cc37d7498c792d9f3f97332cbb25
ADD --checksum=sha256:${AENEAS_SHA256} https://github.com/AeneasVerif/aeneas/releases/download/${AENEAS_RELEASE}/aeneas-linux-x86_64.tar.gz /tmp/aeneas.tar.gz
RUN mkdir -p /opt/aeneas && tar -xzf /tmp/aeneas.tar.gz -C /opt/aeneas && rm /tmp/aeneas.tar.gz \
    && install -m755 /opt/aeneas/aeneas /usr/local/bin/aeneas \
    && aeneas -version

# The Lean dependency graph, pinned by lake-manifest.json. mathlib comes as
# prebuilt oleans from its cache; the Aeneas library builds on top. CI=1
# keeps Aeneas from precompiling modules (its lakefile's own CI setting).
FROM tools AS deps
ENV CI=1
WORKDIR /work/proofs
COPY proofs/lean-toolchain proofs/lakefile.toml proofs/lake-manifest.json ./
RUN --mount=type=cache,target=/root/.cache/mathlib,sharing=locked \
    lake exe cache get \
    && mkdir -p Extracted Kernel && printf 'import Aeneas\n' > Audit.lean \
    && lake build Audit \
    && rm -f Audit.lean

# The gate: the kernel crate, the committed translation and the theorems.
# Translating the real kernel once at build time warms Charon's sysroot
# cache and proves the pipeline runs; the gate itself runs at container
# start (proofs/gate.sh) so its output and timing are visible.
FROM deps AS gate
WORKDIR /work
COPY Cargo.toml ./Cargo.toml
COPY crates/kernel ./crates/kernel
COPY proofs ./proofs
RUN sh proofs/extract.sh crates/kernel /tmp/warm && rm -rf /tmp/warm
CMD ["sh", "proofs/gate.sh"]
