# syntax=docker/dockerfile:1
FROM docker.io/library/rust:1.98.0-alpine3.24@sha256:a10e64dd139b7387337c7fbe8aca31b959b57b2fd4c8ae20a02cf1d6ea424dce AS builder

ARG CARGO_BUILD_FLAGS="--locked --release"
ARG HUBUUM_CLI_BUILD_CHANNEL="dev"
ARG HUBUUM_CLI_BUILD_GIT_SHA=""

WORKDIR /usr/src/hubuum-cli

# Alpine's native Rust target uses musl. The C toolchain and CMake build the
# statically linked cryptography used by rustls.
RUN apk add --no-cache build-base cmake

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
