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
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    CARGO_BUILD_JOBS=4 \
    CARGO_INCREMENTAL=1 \
    cargo build --release --locked --bin leadsnebula-api --bin run-migrations --bin create-user --bin update-password

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

# Copy binaries from builder
COPY --from=builder /app/target/release/leadsnebula-api /app/leadsnebula-api
COPY --from=builder /app/target/release/run-migrations /app/run-migrations
COPY --from=builder /app/target/release/create-user /app/create-user
COPY --from=builder /app/target/release/update-password /app/update-password

# Copy migrations
COPY --from=builder /app/migrations /app/migrations

# Change ownership
RUN chown -R appuser:appuser /app

# Switch to non-root user
USER appuser

# Expose port
EXPOSE 8080

# Health check
HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/health || exit 1

# Run the application
CMD ["/app/leadsnebula-api"]

