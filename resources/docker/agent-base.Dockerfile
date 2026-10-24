# syntax=docker/dockerfile:1
#
# robi/agent-base: the default image for container-sandbox agent executions.
#
# Build from the repository root. The context is the Dockerfile's directory
# because the image copies nothing in, and a repo-root context would send
# `target/` and `node_modules/` to the daemon:
#
#   docker build -t robi/agent:1 \
#     -f resources/docker/agent-base.Dockerfile resources/docker
#
# The image is the PATH contract from docs/src/design/tools/container-sandbox.md.
# A tool the agent needs belongs here, not in the sandbox profile. The harness
# runs the container as the invoking host UID, mounts the workspace at
# /workspace, and keeps /home/agent plus the package caches on named volumes, so
# nothing here may assume a fixed UID or pre-populate those paths.
#
# The root filesystem is writable at runtime. A command may `apt-get install` or
# `pip install`; the write lands in the container's overlay layer, not in this
# image, and is discarded when the container is recreated.

FROM ubuntu:24.04

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

ENV DEBIAN_FRONTEND=noninteractive \
    LANG=C.UTF-8 \
    LC_ALL=C.UTF-8

# System packages are the floor: a shell, a way to fetch things, version
# control, search, archive and diff tools, a C toolchain for native builds, and
# the three language runtimes whose caches the sandbox declares volumes for.
# `passwd` is here for groupadd and useradd, which create the agent account
# below; the ubuntu base image does not include them.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        curl \
        wget \
        gnupg \
        git \
        openssh-client \
        ripgrep \
        fd-find \
        bat \
        grep \
        sed \
        gawk \
        findutils \
        tree \
        file \
        less \
        nano \
        coreutils \
        tar \
        gzip \
        bzip2 \
        xz-utils \
        zstd \
        unzip \
        zip \
        jq \
        patch \
        diffutils \
        build-essential \
        pkg-config \
        libssl-dev \
        python3 \
        python3-pip \
        python3-venv \
        nodejs \
        npm \
        iproute2 \
        iputils-ping \
        dnsutils \
        netcat-openbsd \
        socat \
        procps \
        psmisc \
        htop \
        passwd \
        libgtk-3-dev \
        libwebkit2gtk-4.1-dev \
        libayatana-appindicator3-dev \
        librsvg2-dev \
    && ln -sf /usr/bin/fdfind /usr/local/bin/fd \
    && ln -sf /usr/bin/batcat /usr/local/bin/bat \
    && rm -rf /var/lib/apt/lists/*

# Rust lives at fixed system paths, matching RUSTUP_HOME and the
# /usr/local/cargo/bin entry in the runtime PATH. CARGO_HOME is /usr/local/cargo
# only while building; at runtime the harness points it at the cache volume.
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    RUST_VERSION=stable

# The container runs as an arbitrary host UID, so rustup cannot assume it owns
# RUSTUP_HOME. Keep the shims readable and let the invoking user add components
# or toolchains; the sandbox still drops every capability.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --no-modify-path --profile minimal --default-toolchain "${RUST_VERSION}" \
    && chmod -R a+rwX /usr/local/rustup \
    && chmod -R a+rX /usr/local/cargo \
    && /usr/local/cargo/bin/rustc --version \
    && /usr/local/cargo/bin/cargo --version

# pnpm is not in Ubuntu's archive, and Corepack's bundled version is not
# guaranteed by the `nodejs` package, so install it from npm.
#
# PNPM_HOME is /home/agent/.local/bin, which the harness already puts first on
# PATH, so `pnpm add -g` needs no shell setup. The store stays under
# /home/agent — the home volume — so installs survive a container recreation.
ENV PNPM_VERSION=10 \
    PNPM_HOME=/home/agent/.local/bin

RUN npm install -g "pnpm@${PNPM_VERSION}" \
    && mkdir -p "${PNPM_HOME}" \
    && pnpm --version \
    && node --version

# A named account, not a fixed identity: the container is created with the
# host's UID:GID, and a preparation exec chowns /home/agent to that owner. The
# account exists so HOME resolves and `whoami` is not root when the image is
# inspected or run by hand.
#
# This runs before the runtime ENV below on purpose, and calls the tools by
# absolute path. groupadd and useradd live in /usr/sbin, which the runtime PATH
# intentionally leaves out, so a bare `groupadd` fails with exit 127 here.
RUN test -x /usr/sbin/groupadd \
    && test -x /usr/sbin/useradd \
    && /usr/sbin/groupadd --gid 10001 agent \
    && /usr/sbin/useradd --uid 10001 --gid agent --create-home \
        --home-dir /home/agent --shell /bin/bash agent \
    && mkdir -p /workspace /home/agent/.cargo /home/agent/.cache \
    && chown -R agent:agent /home/agent

# The runtime environment. The harness replaces HOME, CARGO_HOME, and the cache
# variables with mounted volumes; these values keep an interactive `docker run`
# usable without one. /usr/sbin is left out on purpose: a sandboxed command has
# no use for the account-management tools.
ENV HOME=/home/agent \
    USER=agent \
    LOGNAME=agent \
    PATH=/home/agent/.local/bin:/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin \
    TMPDIR=/tmp \
    XDG_CACHE_HOME=/home/agent/.cache \
    CARGO_HOME=/home/agent/.cargo

WORKDIR /workspace

USER agent

# The harness sets its own command; this only makes a bare `docker run` stay up
# long enough to exec into.
CMD ["sleep", "infinity"]
