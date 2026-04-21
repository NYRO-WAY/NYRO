ARG DEV_IMAGE=mcr.microsoft.com/devcontainers/rust:1-1-bookworm
ARG PNPM_VERSION=10.30.3

FROM ${DEV_IMAGE}

ARG PNPM_VERSION

RUN apt-get update && \
    apt-get install -y --no-install-recommends libssl-dev pkg-config libdbus-1-dev python3 python3-pip && \
    rm -rf /var/lib/apt/lists/*

RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - && \
    apt-get update && \
    apt-get install -y --no-install-recommends nodejs && \
    rm -rf /var/lib/apt/lists/*

RUN corepack enable && corepack prepare pnpm@${PNPM_VERSION} --activate

WORKDIR /workspace/nyro
