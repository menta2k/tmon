# tmon task runner — https://github.com/casey/just
set shell := ["bash", "-cu"]

default:
    @just --list

build-ui:
    cd frontend && pnpm install --frozen-lockfile && pnpm build

build: build-ui
    cd backend && cargo build --release --features tmon/embed-ui

dev:
    cd backend && cargo run -p tmon -- serve --config ../deploy/tmon.example.toml

test:
    cd backend && cargo test --workspace
    cd frontend && pnpm test:unit

lint:
    cd backend && cargo fmt --all -- --check && cargo lint
    cd frontend && pnpm lint && pnpm type-check

coverage:
    cd backend && cargo build -q -p tmon-dpi-client --example fake_dpi \
        && TMON_FAKE_DPI="$PWD/target/debug/examples/fake_dpi" cargo llvm-cov --workspace --fail-under-lines 80
    cd frontend && pnpm test:unit --coverage

replay FILE:
    cd backend && cargo run -p tmon -- replay {{FILE}} --to 127.0.0.1:2055 --speed max

# Regenerate the capture (TZSP) fixture pcaps in backend/tests/fixtures/capture/.
fixtures-capture:
    cd backend && cargo run -p tmon-capture --example gen_capture_fixtures --features fixtures

# Run every capture fuzz target for SECS seconds each (nightly + cargo-fuzz).
fuzz-capture SECS="300":
    cd backend/fuzz && for t in capture_datagram tls_client_hello quic_initial dns_response; do \
        cargo +nightly fuzz run $t -- -max_total_time={{SECS}} || exit 1; done

# --- application classification (feature 003) ---------------------------------
NDPI_TAG := "6.0"
NDPI_PREFIX := justfile_directory() + "/backend/dpi/target/ndpi"

# Build the pinned nDPI into backend/dpi/target/ndpi, then the tmon-dpi sidecar.
dpi-build:
    #!/usr/bin/env bash
    set -euo pipefail
    src=backend/dpi/target/ndpi-src
    if [ ! -f "{{NDPI_PREFIX}}/lib/pkgconfig/libndpi.pc" ]; then
        [ -d "$src" ] || git clone --depth 1 --branch {{NDPI_TAG}} https://github.com/ntop/nDPI.git "$src"
        (cd "$src" && ./autogen.sh >/dev/null && ./configure --with-only-libndpi --prefix="{{NDPI_PREFIX}}" >/dev/null \
            && make -j"$(nproc)" -C src/lib >/dev/null && make -C src/lib install >/dev/null && make install-pkgconfigDATA >/dev/null \
            && mkdir -p "{{NDPI_PREFIX}}/share/doc/ndpi" && cp COPYING "{{NDPI_PREFIX}}/share/doc/ndpi/")
    fi
    PKG_CONFIG_PATH="{{NDPI_PREFIX}}/lib/pkgconfig" cargo build --release --manifest-path backend/dpi/Cargo.toml

# Test the sidecar workspace against the locally built nDPI.
test-dpi:
    PKG_CONFIG_PATH="{{NDPI_PREFIX}}/lib/pkgconfig" LD_LIBRARY_PATH="{{NDPI_PREFIX}}/lib" \
        cargo test --manifest-path backend/dpi/Cargo.toml

# Regenerate the application fixture pcaps (BitTorrent, WireGuard, ...).
fixtures-apps:
    cd backend && cargo run -p tmon-capture --example gen_app_fixtures --features fixtures

# Fuzz the IPC codec (both directions) and the sidecar packet path for SECS seconds each.
fuzz-dpi SECS="300":
    cd backend/fuzz && for t in dpi_proto_to_sidecar dpi_proto_to_tmon; do \
        cargo +nightly fuzz run $t -- -max_total_time={{SECS}} || exit 1; done
    cd backend/dpi/fuzz && PKG_CONFIG_PATH="{{NDPI_PREFIX}}/lib/pkgconfig" LD_LIBRARY_PATH="{{NDPI_PREFIX}}/lib" \
        cargo +nightly fuzz run sidecar_packets -- -max_total_time={{SECS}}

# Container images: the base image (no nDPI) and the -dpi variant.
docker:
    docker build -f deploy/Dockerfile -t tmon .

docker-dpi:
    #!/usr/bin/env bash
    set -euo pipefail
    docker build -f deploy/Dockerfile --target tmon-dpi -t tmon:dpi .
    docker build -f deploy/Dockerfile -t tmon .
    # The base image must never contain nDPI (distroless: no shell, so export).
    id=$(docker create tmon); trap 'docker rm -f "$id" >/dev/null' EXIT
    if docker export "$id" | tar -t | grep -q libndpi; then echo "libndpi in base image" >&2; exit 1; fi
    echo "base image has no libndpi"
