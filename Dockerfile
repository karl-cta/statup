# Binary
FROM rust:1.93-alpine AS builder

RUN apk add --no-cache musl-dev

WORKDIR /app

COPY Cargo.toml Cargo.lock ./

# Builds the dependencies alone, for the layer cache. The stub is not the
# crate, so this step may fail without consequence: it only warms the cache.
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    (cargo build --release --locked || true) && \
    rm -rf src

COPY src ./src
COPY templates ./templates
COPY locales ./locales
COPY migrations ./migrations

# Newer than the stub, so cargo rebuilds the crate itself.
RUN touch src/main.rs src/lib.rs && cargo build --release --locked

# Stylesheet, built apart so a change in static/ does not rebuild the binary
FROM alpine:3.21 AS styles

# Tailwind CSS v4 standalone CLI, the musl build since the image is Alpine.
# It links the C++ runtime dynamically.
RUN apk add --no-cache libstdc++ libgcc

ARG TARGETARCH
RUN case "${TARGETARCH:-amd64}" in \
      amd64) TW_ARCH=x64 ;; \
      arm64) TW_ARCH=arm64 ;; \
      *) echo "unsupported arch: ${TARGETARCH}" && exit 1 ;; \
    esac && \
    wget -O /usr/local/bin/tailwindcss \
      "https://github.com/tailwindlabs/tailwindcss/releases/download/v4.1.18/tailwindcss-linux-${TW_ARCH}-musl" && \
    chmod +x /usr/local/bin/tailwindcss

WORKDIR /app

# Tailwind reads the class names used in the templates and the scripts.
COPY static ./static
COPY templates ./templates

# Only the built stylesheet is served, never its sources.
RUN tailwindcss --input static/css/input.css --output static/css/style.css --minify && \
    find static/css -name '*.css' ! -name style.css -delete && \
    find static/css -mindepth 1 -type d -exec rm -rf {} +

# Runtime
FROM alpine:3.21

# A fixed id, so a bind-mounted data directory can be given to it.
RUN addgroup -S -g 10001 statup && \
    adduser -S -D -H -u 10001 -G statup -s /sbin/nologin statup && \
    mkdir -p /data && chown statup:statup /data

WORKDIR /app

COPY --from=builder /app/target/release/statup /app/statup
COPY --from=styles /app/static /app/static
COPY LICENSE THIRD_PARTY_NOTICES.md /app/

# Uploaded icons live in the data volume beside the database, where the app
# user may write and where an image update does not wipe them.
ENV DATABASE_URL=/data/statup.db
ENV UPLOAD_DIR=/data/uploads
ENV HOST=0.0.0.0
ENV PORT=3000

USER statup

EXPOSE 3000

VOLUME ["/data"]

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD wget -q --spider http://127.0.0.1:3000/health || exit 1

ENTRYPOINT ["/app/statup"]
