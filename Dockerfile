# Build stage
FROM rust:1.84-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app

# Download tailwindcss standalone CLI, multi-arch
ARG TARGETARCH
RUN case "${TARGETARCH:-amd64}" in \
      amd64) TW_ARCH=x64 ;; \
      arm64) TW_ARCH=arm64 ;; \
      *) echo "unsupported arch: ${TARGETARCH}" && exit 1 ;; \
    esac && \
    wget -O /usr/local/bin/tailwindcss \
      "https://github.com/tailwindlabs/tailwindcss/releases/download/v3.4.17/tailwindcss-linux-${TW_ARCH}" && \
    chmod +x /usr/local/bin/tailwindcss

# Copy manifests first for dependency caching
COPY Cargo.toml Cargo.lock ./

# Create dummy src to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release 2>/dev/null || true
RUN rm -rf src

# Copy actual source code
COPY src ./src
COPY templates ./templates
COPY migrations ./migrations
COPY static ./static

# Build Tailwind CSS (minified)
RUN tailwindcss --input static/css/input.css --output static/css/style.css --minify

# Build the application
RUN cargo build --release

# Runtime stage
FROM alpine:3.21

RUN apk add --no-cache ca-certificates wget

WORKDIR /app

RUN addgroup -S statup && adduser -S statup -G statup && \
    mkdir -p /data && chown statup:statup /data

COPY --from=builder --chown=statup:statup /app/target/release/statup /app/statup
COPY --from=builder --chown=statup:statup /app/static /app/static
COPY --from=builder --chown=statup:statup /app/migrations /app/migrations

ENV DATABASE_URL=/data/statup.db
ENV HOST=0.0.0.0
ENV PORT=3000

USER statup

EXPOSE 3000

VOLUME ["/data"]

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget --no-verbose --tries=1 --spider http://localhost:3000/health || exit 1

ENTRYPOINT ["/app/statup"]
