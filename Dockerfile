# syntax=docker/dockerfile:1

# ---- Stage 1: build ----------------------------------------------------------
# Must be new enough for the versions pinned in Cargo.lock.
FROM rust:1.94-slim-bookworm AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

# The binary is copied out inside this RUN because cache mounts do not persist
# into the resulting layer.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked --bin capsolver && \
    cp target/release/capsolver /app/capsolver

# ---- Stage 2: runtime --------------------------------------------------------
FROM debian:bookworm-slim

WORKDIR /app

# curl is required by HEALTHCHECK below; tini reaps the helper processes Chrome
# forks, which would otherwise pile up as zombies under PID 1.
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
        curl \
        tini \
        wget \
        gnupg \
        fonts-liberation \
        fontconfig \
    && rm -rf /var/lib/apt/lists/*

# apt-key is deprecated and removed in newer Debian, so the key goes to a keyring
# that the source entry references directly.
RUN wget -qO- https://dl-ssl.google.com/linux/linux_signing_key.pub \
        | gpg --dearmor -o /usr/share/keyrings/google-chrome.gpg && \
    echo "deb [arch=amd64 signed-by=/usr/share/keyrings/google-chrome.gpg] http://dl.google.com/linux/chrome/deb/ stable main" \
        > /etc/apt/sources.list.d/google-chrome.list && \
    apt-get update && \
    apt-get install -y --no-install-recommends google-chrome-stable && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/capsolver /app/capsolver

# Chrome cannot run as root without --no-sandbox, and running unprivileged is
# preferable regardless. HOME must be writable for Chrome's crash handler.
RUN useradd --create-home --uid 1000 capsolver && \
    chown -R capsolver:capsolver /app

ENV HOME=/home/capsolver \
    CHROME_PATH=/usr/bin/google-chrome \
    CHROME_BIN=/usr/bin/google-chrome \
    PORT=407 \
    SERVER_HOST=0.0.0.0 \
    BROWSER_POOL_SIZE=2 \
    TABS_PER_PROCESS=10 \
    # Timeouts are milliseconds.
    SOLVE_TIMEOUT=29000 \
    LOAD_TIMEOUT=30000 \
    CDP_TIMEOUT=10000 \
    DISABLE_SANDBOX=true \
    LOG_LEVEL=info \
    RUST_LOG=info

EXPOSE 407

USER capsolver

HEALTHCHECK --interval=30s --timeout=10s --start-period=40s --retries=3 \
    CMD curl -fsS http://127.0.0.1:${PORT}/health || exit 1

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/app/capsolver"]
