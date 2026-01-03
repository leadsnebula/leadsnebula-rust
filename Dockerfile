# Multi-stage Dockerfile optimized for Rust deployments on Fly.io
# Uses best practices: layer caching, minimal runtime image, proper dependency management

# ============================================================================
# Build stage - use Debian bookworm to match runtime
# ============================================================================
FROM debian:bookworm-slim AS builder

# Install Rust and build dependencies in a single layer for better caching
RUN apt-get update && apt-get install -y --no-install-recommends \
    curl \
    ca-certificates \
    build-essential \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Install Rust (minimal profile for faster installs)
# Source the cargo env to make rustup available immediately
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal \
    && . "$HOME/.cargo/env" \
    && rustup default stable

ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /app

# Copy dependency manifests first for better layer caching
# This allows Docker to cache dependencies separately from source code
COPY Cargo.toml Cargo.lock* ./

# Copy workspace structure
COPY crates ./crates
COPY migrations ./migrations

# Build the application, migration runner, and utility binaries
# Cargo will generate Cargo.lock if it doesn't exist
RUN cargo build --release --bin leadsnebula-api --bin run-migrations --bin create-user --bin update-password

# ============================================================================
# Runtime stage - minimal image with only what's needed
# ============================================================================
FROM debian:bookworm-slim

# Install only runtime dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binaries from builder
COPY --from=builder /app/target/release/leadsnebula-api /app/leadsnebula-api
COPY --from=builder /app/target/release/run-migrations /app/run-migrations
COPY --from=builder /app/target/release/create-user /app/create-user
COPY --from=builder /app/target/release/update-password /app/update-password

# Copy migrations directory
COPY --from=builder /app/migrations /app/migrations

# Create non-root user for security (best practice)
RUN useradd -m -u 1000 appuser && \
    chown -R appuser:appuser /app

USER appuser

# Expose port
EXPOSE 8080

# Health check - use curl if available, otherwise skip
# Note: Health checks are better configured in fly.toml

# Run the application
CMD ["/app/leadsnebula-api"]
