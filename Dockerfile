# syntax=docker/dockerfile:1

# =========================================================
# Stage 1: Minimal non-root Alpine runtime base
# =========================================================
FROM alpine:3.24 AS runtime-base

RUN apk add --no-cache ca-certificates \
    && addgroup -g 1000 -S toolstow \
    && adduser -u 1000 -S -G toolstow -h /data -D toolstow \
    && mkdir -p /data \
    && chown -R toolstow:toolstow /data

LABEL org.opencontainers.image.title="ToolStow" \
      org.opencontainers.image.description="Single-binary self-hosted personal micro-tools host" \
      org.opencontainers.image.source="https://github.com/Pyaxy/ToolStow" \
      org.opencontainers.image.licenses="MIT"

ENV TOOLSTOW_HOST=0.0.0.0 \
    TOOLSTOW_PORT=8080 \
    TOOLSTOW_DATA_DIR=/data

WORKDIR /data
VOLUME ["/data"]
EXPOSE 8080

USER 1000:1000

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD sh -c 'wget -q --spider "http://127.0.0.1:${TOOLSTOW_PORT:-8080}/healthz"' || exit 1

ENTRYPOINT ["/usr/local/bin/toolstow"]

# =========================================================
# Stage 2: Fast CI multi-arch stage (reuses static musl binaries)
# =========================================================
FROM runtime-base AS prebuilt

ARG TARGETARCH
COPY --chmod=0755 dist-docker/${TARGETARCH}/toolstow /usr/local/bin/toolstow

# =========================================================
# Stage 3: Local source build stage
# =========================================================
FROM rust:1-alpine AS builder

WORKDIR /src
RUN apk add --no-cache musl-dev

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY frontend ./frontend
COPY admin ./admin
COPY ui ./ui

RUN cargo build --release --locked \
    && chmod 0755 /src/target/release/toolstow

# =========================================================
# Stage 4: Default final image from local source build
# =========================================================
FROM runtime-base AS runtime

COPY --from=builder --chmod=0755 /src/target/release/toolstow /usr/local/bin/toolstow
