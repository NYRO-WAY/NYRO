ARG RUST_IMAGE=rust:1.88-bookworm
ARG NODE_IMAGE=node:20-bookworm
ARG RUNTIME_IMAGE=debian:bookworm-slim
ARG PNPM_VERSION=10.30.3

FROM ${NODE_IMAGE} AS web-builder
WORKDIR /workspace/webui
ARG PNPM_VERSION

RUN corepack enable && corepack prepare pnpm@${PNPM_VERSION} --activate

COPY webui/package.json webui/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile

COPY webui/ ./
RUN pnpm build

FROM ${RUST_IMAGE} AS rust-builder
WORKDIR /workspace

RUN apt-get update && \
    apt-get install -y --no-install-recommends pkg-config libssl-dev libdbus-1-dev ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY Cargo.toml ./
COPY crates ./crates
COPY src-server ./src-server
COPY src-tauri/Cargo.toml ./src-tauri/Cargo.toml
COPY src-tauri/build.rs ./src-tauri/build.rs
COPY src-tauri/src ./src-tauri/src

RUN mkdir -p /workspace/webui/dist/ && cargo build --release -p nyro-server

FROM ${RUNTIME_IMAGE} AS runtime
WORKDIR /app

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl tzdata libdbus-1-3 && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --system nyro && \
    useradd --system --gid nyro --home-dir /var/lib/nyro --create-home --shell /usr/sbin/nologin nyro && \
    mkdir -p /app/webui/dist /var/lib/nyro && \
    chown -R nyro:nyro /app /var/lib/nyro

COPY --from=rust-builder /workspace/target/release/nyro-server /app/nyro-server
COPY --from=web-builder /workspace/webui/dist /app/webui/dist
COPY docker/runtime/docker-entrypoint.sh /app/docker-entrypoint.sh

RUN chmod +x /app/docker-entrypoint.sh && \
    chown nyro:nyro /app/docker-entrypoint.sh

USER nyro

ENV NYRO_DATA_DIR=/var/lib/nyro

EXPOSE 19530 19531

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
  CMD curl --fail --silent http://127.0.0.1:19530/health > /dev/null || exit 1

ENTRYPOINT ["/app/docker-entrypoint.sh"]
CMD []
