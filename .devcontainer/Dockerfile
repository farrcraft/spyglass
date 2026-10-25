# One image definition for the dev container and for `cargo xtask linux-check`.
# See docs/adr/0007-dev-container-and-linux-check.md.

# What docs/getting-started.md assumes a clean Linux machine has. No Rust and no Java:
# scripts/setup.sh installs Rust, and linux-check installs Java itself.
FROM ubuntu:24.04 AS base
ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential \
        ca-certificates \
        curl \
        git \
    && rm -rf /var/lib/apt/lists/*

# The dev container. Rust, the cargo tools and pre-commit come from scripts/setup.sh, run as the
# post-create command, so every rebuild exercises it.
FROM base AS dev
ARG DEBIAN_FRONTEND=noninteractive
# The JDK major must match the target game version's javaVersion.majorVersion (25 for 26.3).
# A JDK serves both the extractor and the oracle. The rest are the viewer's build dependencies.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        libvulkan-dev \
        libwayland-dev \
        libx11-dev \
        libxcursor-dev \
        libxi-dev \
        libxkbcommon-dev \
        libxrandr-dev \
        openjdk-25-jdk-headless \
        sudo \
    && rm -rf /var/lib/apt/lists/* \
    && echo 'ubuntu ALL=(ALL) NOPASSWD:ALL' > /etc/sudoers.d/ubuntu \
    && chmod 0440 /etc/sudoers.d/ubuntu
# Create the volume mount points owned by the user, so Docker gives new volumes that ownership.
RUN mkdir -p /home/ubuntu/.cargo /home/ubuntu/.rustup /home/ubuntu/target \
    && chown -R ubuntu:ubuntu /home/ubuntu
USER ubuntu
ENV PATH=/home/ubuntu/.cargo/bin:/home/ubuntu/.local/bin:$PATH
