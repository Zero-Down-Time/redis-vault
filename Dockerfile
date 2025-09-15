# Dockerfile
# Build arguments for version control
ARG RUST_VERSION=1.87
ARG ALPINE_VERSION=3.22

# Builder stage - using Alpine-based Rust for smaller layers
FROM rust:${RUST_VERSION}-alpine as builder

# Install build dependencies
RUN apk add --no-cache musl-dev

WORKDIR /app

# Copy manifests
COPY Cargo.toml Cargo.lock ./

# Build dependencies (this is cached as long as Cargo.toml doesn't change)
RUN mkdir src && \
    echo "fn main() {}" > src/main.rs && \
    cargo build --release && \
    rm -rf src

# Copy source code
COPY src ./src

# Build application with static linking for Alpine
RUN cargo build --release --target-dir=./target

# Runtime stage
FROM alpine:${ALPINE_VERSION}

# Install required runtime dependencies only
# libgcc is needed for Rust panic handling
# libssl3 is for TLS connections to cloud providers
RUN apk add --no-cache \
    ca-certificates \
    libgcc \
    libssl3 && \
    rm -rf /var/cache/apk/*

# Copy binary from builder
COPY --from=builder /app/target/release/redis-vault /usr/local/bin/redis-vault

# Create non-root user and group
RUN addgroup -g 1000 vault && \
    adduser -D -u 1000 -G vault vault && \
    mkdir -p /data && \
    chown vault:vault /data

USER vault

ENTRYPOINT ["redis-vault"]
