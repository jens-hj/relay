set shell := ["bash", "-euo", "pipefail", "-c"]
set dotenv-load
set positional-arguments

# Show available development commands.
default:
    @just --list

# Build and launch the local server and desktop; optionally choose another port.
dev port="7331":
    #!/usr/bin/env bash
    set -euo pipefail
    relay_port="$1"
    if ! [[ "$relay_port" =~ ^[0-9]{1,5}$ ]] || ((10#$relay_port < 1 || 10#$relay_port > 65535)); then
        printf 'Port must be a number from 1 to 65535.\n' >&2
        exit 1
    fi

    cargo build --locked --workspace
    relay_target_dir="$(cargo metadata --locked --no-deps --format-version 1 | jq -r '.target_directory')"
    export RELAY_TOKEN="${RELAY_TOKEN:-$(openssl rand -hex 32)}"
    export RELAY_BIND="127.0.0.1:$relay_port"
    export RELAY_ENDPOINT="http://$RELAY_BIND"

    if (exec 3<>"/dev/tcp/127.0.0.1/$relay_port") 2>/dev/null; then
        printf 'Local port %s is already in use; choose another with just dev PORT.\n' "$relay_port" >&2
        exit 1
    fi

    relay_server_pid=""
    relay_client_pid=""
    cleanup() {
        for relay_pid in "$relay_client_pid" "$relay_server_pid"; do
            if [[ -n "$relay_pid" ]]; then
                kill "$relay_pid" 2>/dev/null || true
                wait "$relay_pid" 2>/dev/null || true
            fi
        done
    }
    trap cleanup EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM

    "$relay_target_dir/debug/relay-server" &
    relay_server_pid=$!
    relay_ready=false
    for ((relay_attempt = 0; relay_attempt < 100; relay_attempt++)); do
        if ! kill -0 "$relay_server_pid" 2>/dev/null; then
            wait "$relay_server_pid" || true
            relay_server_pid=""
            printf 'Relay server failed to start.\n' >&2
            exit 1
        fi
        if curl --fail --silent --output /dev/null --connect-timeout 1 --max-time 1 \
            --header "Authorization: Bearer $RELAY_TOKEN" "$RELAY_ENDPOINT/v1/snapshot"; then
            relay_ready=true
            break
        fi
        sleep 0.1
    done
    if [[ "$relay_ready" != true ]]; then
        printf 'Timed out waiting for the local Relay server.\n' >&2
        exit 1
    fi

    "$relay_target_dir/debug/relay-desktop" &
    relay_client_pid=$!
    relay_status=0
    wait "$relay_client_pid" || relay_status=$?
    relay_client_pid=""
    exit "$relay_status"

# Run only the server; set RELAY_TOKEN in the environment or .env.
server:
    cargo run --locked -p relay-server

# Connect a desktop to an existing server with RELAY_TOKEN and RELAY_ENDPOINT.
client:
    cargo run --locked -p relay-desktop

# Build the entire workspace.
build:
    cargo build --locked --workspace

# Run all domain, server, and headless UI tests.
test:
    cargo test --locked --workspace

# Check Rust diagnostics with warnings treated as errors.
lint:
    cargo clippy --locked --workspace --all-targets -- -D warnings

# Format Rust, Mosaic views, and Nix files.
fmt:
    just --fmt
    cargo fmt --all
    nix run .#mosaic-fmt -- crates/relay-desktop/src
    nix fmt

# Verify formatting without changing files.
fmt-check:
    just --fmt --check
    cargo fmt --all -- --check
    nix run .#mosaic-fmt -- --check crates/relay-desktop/src
    nix fmt -- --fail-on-change

# Evaluate the pinned flake.
nix-check:
    nix flake check --no-build

# Run the project's validation commands.
check: test lint fmt-check nix-check
