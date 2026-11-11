# syntax=docker/dockerfile:1
FROM docker.io/library/rust:1.99.1-alpine3.24@sha256:b1d5dd5a22a09404d3e798c0cf051b689077360374eb505fa5bb93c479693d87 AS builder

ARG CARGO_BUILD_FLAGS="--locked --release"
ARG HUBUUM_CLI_BUILD_CHANNEL="dev"
ARG HUBUUM_CLI_BUILD_GIT_SHA=""

WORKDIR /usr/src/hubuum-cli

# Alpine's native Rust target uses musl. The C toolchain and CMake build the
# statically linked cryptography used by rustls.
RUN apk add --no-cache build-base cmake

# Rust's musl runtime does not provide GCC's outlined atomic helpers used by
# vendored D-Bus. Keep its atomics inline on the baseline ARMv8 instruction set.
ENV CFLAGS_aarch64_unknown_linux_musl="-mno-outline-atomics"

COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/usr/src/hubuum-cli/target \
    HUBUUM_CLI_BUILD_CHANNEL="${HUBUUM_CLI_BUILD_CHANNEL}" \
    HUBUUM_CLI_BUILD_GIT_SHA="${HUBUUM_CLI_BUILD_GIT_SHA}" \
    cargo build ${CARGO_BUILD_FLAGS} --bin hubuum-cli && \
    cp target/release/hubuum-cli /tmp/hubuum-cli

RUN /tmp/hubuum-cli --version

FROM scratch AS release-artifacts

COPY --from=builder /tmp/hubuum-cli /hubuum-cli
