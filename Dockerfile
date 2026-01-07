# Multi-stage Dockerfile for Rust application
# Optimized for fast builds with cache mounts and parallel compilation

# Builder stage
# Use latest stable Rust to support Cargo.lock version 4
FROM rust:bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Set working directory
WORKDIR /app

# Copy workspace files for dependency caching
COPY Cargo.toml Cargo.lock ./

# Create dummy source structure for dependency caching
RUN mkdir -p crates/api/src crates/core/src crates/utils/src/bin && \
    echo "fn main() {}" > crates/api/src/main.rs && \
    echo "pub fn lib() {}" > crates/core/src/lib.rs

# Copy crate manifests
COPY crates/api/Cargo.toml crates/api/
COPY crates/core/Cargo.toml crates/core/
COPY crates/utils/Cargo.toml crates/utils/

# Fetch dependencies (cached layer)
RUN cargo fetch --locked

# Copy actual source code
COPY crates/ ./crates/
COPY migrations/ ./migrations/

# Build with cache mounts for faster rebuilds
# Note: Cache mounts are ephemeral, so we copy binaries to persistent location after build
# Optimizations:
# - Use all available CPU cores (detected automatically, but limit to 4 for consistency)
# - Enable incremental compilation for faster rebuilds
# - Build only required binaries to reduce build time
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    --mount=type=cache,target=/usr/local/cargo/git \
    CARGO_BUILD_JOBS=4 \
    CARGO_INCREMENTAL=1 \
    RUSTFLAGS="-C target-cpu=native" \
    cargo build --release --locked --bin leadsnebula-api --bin run-migrations --bin create-user --bin update-password --bin test-redis-connection && \
    mkdir -p /app/binaries && \
    cp /app/target/release/leadsnebula-api /app/binaries/ && \
    cp /app/target/release/run-migrations /app/binaries/ && \
    cp /app/target/release/create-user /app/binaries/ && \
    cp /app/target/release/update-password /app/binaries/ && \
    cp /app/target/release/test-redis-connection /app/binaries/

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN useradd -m -u 1000 appuser

# Set working directory
WORKDIR /app

# Copy binaries from builder (from persistent location, not cache mount)
COPY --from=builder /app/binaries/leadsnebula-api /app/leadsnebula-api
COPY --from=builder /app/binaries/run-migrations /app/run-migrations
COPY --from=builder /app/binaries/create-user /app/create-user
COPY --from=builder /app/binaries/update-password /app/update-password
COPY --from=builder /app/binaries/test-redis-connection /app/test-redis-connection

# Copy migrations
COPY --from=builder /app/migrations /app/migrations

# Change ownership
RUN chown -R appuser:appuser /app

# Switch to non-root user
USER appuser

# Expose port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=20s --retries=3 \
    CMD curl -f http://localhost:8080/live || exit 1

# Run the application
CMD ["/app/leadsnebula-api"]

