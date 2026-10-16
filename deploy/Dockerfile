# syntax=docker/dockerfile:1.4
# ==============================================================================
# Multi-Stage Production Dockerfile for TruLang Cloud Microservices
# Stage 1: Builder compiles the service to a statically-linked native executable
# Stage 2: Minimal runtime image (Alpine Linux, non-root, < 15MB total footprint)
# ==============================================================================

FROM rust:1.80-alpine AS builder

RUN apk add --no-cache musl-dev gcc make

WORKDIR /workspace

# Copy workspace manifests
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

# Build release TruLang compiler and runtime CLI
RUN cargo build --release -p trulang_cli

# Compile sample microservice into standalone AOT native binary
COPY examples/http_api_gateway.tr ./service.tr
RUN ./target/release/trulang build --release service.tr -o /workspace/service_bin

# ==============================================================================
# Stage 2: Minimal Distroless / Hardened Alpine Runtime
# ==============================================================================
FROM alpine:3.20 AS runtime

# Install CA certificates and minimal runtime dependencies
RUN apk add --no-cache ca-certificates tzdata

# Security: Create non-root dedicated service user
RUN addgroup -g 10001 -S trulang && \
    adduser -u 10001 -S trulang -G trulang

WORKDIR /app

# Copy compiled standalone native binary from builder stage
COPY --from=builder --chown=trulang:trulang /workspace/service_bin /app/server

USER trulang:trulang

EXPOSE 8080

# Kubernetes & Container Engine Health Check
HEALTHCHECK --interval=10s --timeout=3s --start-period=2s --retries=3 \
  CMD wget --no-verbose --tries=1 --spider http://127.0.0.1:8080/healthz || exit 1

# Launch microservice with native signal handling
ENTRYPOINT ["/app/server"]
