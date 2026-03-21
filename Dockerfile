# =============================================================================
# Stage 1: Builder — compile the Rust binary in a full toolchain image
# =============================================================================
FROM rust:1.83-slim-bookworm AS builder

WORKDIR /app

# Install OS-level dependencies needed by some crates (openssl, pkg-config)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy manifests first for better layer caching
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs so cargo can fetch + compile dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs && echo "" > src/lib.rs
RUN cargo build --release && rm -rf src

# Now copy the real source and rebuild (only our crate recompiles)
COPY src/ src/

# Touch main.rs so cargo sees it as newer than the dummy
RUN touch src/main.rs src/lib.rs
RUN cargo build --release

# =============================================================================
# Stage 2: Runtime — slim image with just the binary and static assets
# =============================================================================
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*

# Non-root user for security
RUN useradd -r -s /bin/false appuser

WORKDIR /app

# Copy compiled binary from builder
COPY --from=builder /app/target/release/political-debaiser /app/political-debaiser

# Copy static assets (served by tower-http ServeDir at runtime)
COPY static/ /app/static/

# Own app directory as appuser
RUN chown -R appuser:appuser /app

USER appuser

# Default env: connect to Ollama on Docker host (overridden by compose)
ENV OLLAMA_URL=http://host.docker.internal:11434
ENV OLLAMA_MODEL=llama3.2
ENV RUST_LOG=info
ENV LOG_FORMAT=text

EXPOSE 3000

HEALTHCHECK --interval=15s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:3000/health || exit 1

CMD ["/app/political-debaiser"]
