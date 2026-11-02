# syntax=docker/dockerfile:1.7
# Build from the repository root:
#   docker build -f deploy/Dockerfile -t tmon .                       # base image (no nDPI)
#   docker build -f deploy/Dockerfile --target tmon-dpi -t tmon:dpi . # + application classifier
#
# The base image never contains libndpi: nDPI (LGPL-3.0) is only built in the
# ndpi-build stage and only copied into the `tmon-dpi` target (docs/dpi.md).

ARG NDPI_TAG=6.0
ARG NDPI_REPO=https://github.com/ntop/nDPI.git
# Optional: the full commit the tag must resolve to (guards against a moved tag).
ARG NDPI_COMMIT=

FROM node:22-bookworm-slim AS ui
WORKDIR /src/frontend
RUN corepack enable
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY frontend/ ./
RUN pnpm build

FROM rust:1-bookworm AS backend
WORKDIR /src
RUN apt-get update && apt-get install -y --no-install-recommends cmake clang && rm -rf /var/lib/apt/lists/*
COPY backend/ backend/
COPY --from=ui /src/frontend/dist frontend/dist
WORKDIR /src/backend
RUN cargo build --release --locked --features embed-ui -p tmon \
 && install -D target/release/tmon /out/tmon \
 && mkdir -p /out/data

# --- nDPI (pinned tag, library only) -------------------------------------------
FROM debian:bookworm-slim AS ndpi-build
ARG NDPI_TAG
ARG NDPI_REPO
ARG NDPI_COMMIT
# nDPI parses untrusted packets: build it hardened.
ENV CFLAGS="-O2 -fstack-protector-strong -D_FORTIFY_SOURCE=2 -fPIC" \
    LDFLAGS="-Wl,-z,relro,-z,now"
RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates git build-essential autoconf automake libtool pkg-config \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
RUN git clone --depth 1 --branch "${NDPI_TAG}" "${NDPI_REPO}" ndpi \
 && cd ndpi && git rev-parse HEAD > /ndpi-commit \
 && { [ -z "${NDPI_COMMIT}" ] || [ "$(cat /ndpi-commit)" = "${NDPI_COMMIT}" ] \
      || { echo "nDPI ${NDPI_TAG} is $(cat /ndpi-commit), expected ${NDPI_COMMIT}"; exit 1; }; } \
 && ./autogen.sh && ./configure --with-only-libndpi --prefix=/usr/local \
 && make -j"$(nproc)" -C src/lib && make -C src/lib install && make install-pkgconfigDATA \
 && install -D -m 0644 COPYING /usr/local/share/doc/tmon-dpi/nDPI-COPYING

# --- tmon-dpi sidecar ---------------------------------------------------------
FROM rust:1-bookworm AS dpi-build
ARG NDPI_TAG
ARG NDPI_REPO
RUN apt-get update && apt-get install -y --no-install-recommends clang libclang-dev pkg-config \
 && rm -rf /var/lib/apt/lists/*
COPY --from=ndpi-build /usr/local/ /usr/local/
COPY --from=ndpi-build /ndpi-commit /ndpi-commit
WORKDIR /src
COPY backend/ backend/
WORKDIR /src/backend/dpi
# Distroless has no ldconfig: find libndpi through an rpath instead.
ENV PKG_CONFIG_PATH=/usr/local/lib/pkgconfig \
    RUSTFLAGS="-C link-arg=-Wl,-rpath,/usr/local/lib"
RUN cargo build --release --locked -p tmon-dpi \
 && install -D target/release/tmon-dpi /out/bin/tmon-dpi \
 && ! ldd /out/bin/tmon-dpi | grep -q "not found" \
 && cp -P /usr/local/lib/libndpi.so* /out/ \
 && mkdir -p /out/doc \
 && cp /usr/local/share/doc/tmon-dpi/nDPI-COPYING /out/doc/ \
 && printf '%s\n' \
      "tmon-dpi links nDPI ${NDPI_TAG} (LGPL-3.0-or-later) dynamically." \
      "Source: ${NDPI_REPO} tag ${NDPI_TAG}, commit $(cat /ndpi-commit)." \
      "The complete corresponding source is the public repository above at that" \
      "commit. If you redistribute this image, you must also provide that source" \
      "or a written offer for it yourself (LGPL-3.0 section 4; see docs/dpi.md)." \
      "You may replace /usr/local/lib/libndpi.so.* with a compatible build." \
      > /out/doc/NOTICE-nDPI

# --- runtime images -------------------------------------------------------------
FROM gcr.io/distroless/cc-debian12:nonroot AS runtime
COPY --from=backend /out/tmon /usr/local/bin/tmon
COPY --from=backend --chown=nonroot:nonroot /out/data /var/lib/tmon
COPY deploy/tmon.example.toml /etc/tmon/tmon.toml
USER nonroot:nonroot
VOLUME ["/var/lib/tmon"]
EXPOSE 8080 2055/udp 4739/udp 6343/udp
ENTRYPOINT ["/usr/local/bin/tmon"]
CMD ["serve", "--config", "/etc/tmon/tmon.toml"]

# Base image + the classifier. Enable it with [capture] and [dpi] in tmon.toml
# (binary = "/usr/local/bin/tmon-dpi").
FROM runtime AS tmon-dpi
COPY --from=dpi-build /out/bin/tmon-dpi /usr/local/bin/tmon-dpi
COPY --from=dpi-build /out/libndpi.so* /usr/local/lib/
COPY --from=dpi-build /out/doc/ /usr/share/doc/tmon-dpi/

# Default target (last stage): the base image, unchanged and without nDPI.
FROM runtime AS tmon
