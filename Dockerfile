# Multi-stage Dockerfile for production Rust CAPTCHA Solver service
# Stage 1: Builder
FROM rust:1.75 as builder

WORKDIR /app

# Install dependencies for building
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy Cargo files
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src

# Build optimized binary
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release && \
    mv target/release/capsolver /app/capsolver

# Stage 2: Runtime
FROM debian:bookworm-slim

WORKDIR /app

# Install Chrome and necessary libraries
RUN apt-get update && apt-get install -y \
    wget \
    gnupg \
    ca-certificates \
    fonts-dejavu \
    fontconfig \
    libssl3 \
    libfontconfig1 \
    libfreetype6 \
    libnss3 \
    xdg-utils \
    x11-utils \
    && rm -rf /var/lib/apt/lists/*

# Install Chrome stable
RUN wget -q -O - https://dl-ssl.google.com/linux/linux_signing_key.pub | apt-key add - && \
    echo "deb [arch=amd64] http://dl.google.com/linux/chrome/deb/ stable main" > /etc/apt/sources.list.d/google-chrome.list && \
    apt-get update && \
    apt-get install -y google-chrome-stable && \
    rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /app/capsolver /app/

# Create non-root user for security
RUN useradd -m -u 1000 capsolver && \
    chown -R capsolver:capsolver /app

# Set environment variables
ENV CHROME_BIN=/usr/bin/google-chrome
ENV PORT=407
ENV SERVER_PORT=407
ENV SERVER_HOST=0.0.0.0
ENV BROWSER_POOL_SIZE=2
ENV TABS_PER_PROCESS=10
ENV SOLVE_TIMEOUT=29000
ENV LOG_LEVEL=info
ENV RUST_LOG=info

# Expose port
EXPOSE 407

# Switch to non-root user
USER capsolver

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=40s --retries=3 \
    CMD curl -f http://localhost:407/health || exit 1

# Run the application
CMD ["/app/capsolver"]
