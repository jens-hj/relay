set shell := ["bash", "-euo", "pipefail", "-c"]
set dotenv-load
set positional-arguments

# Show available development commands.
default:
    @just --list

# Check local harness installations/authentication independently.
doctor:
    #!/usr/bin/env bash
    set -euo pipefail
    for relay_harness in codex claude; do
        relay_override="RELAY_CODEX_BIN"
        [[ "$relay_harness" == claude ]] && relay_override="RELAY_CLAUDE_BIN"
        relay_binary="${!relay_override:-}"
        if [[ -z "$relay_binary" ]]; then
            for relay_candidate in "$HOME/.local/bin/$relay_harness" "$HOME/.npm-global/bin/$relay_harness" "$HOME/.nix-profile/bin/$relay_harness"; do
                if [[ -x "$relay_candidate" ]]; then relay_binary="$relay_candidate"; break; fi
            done
        fi
        relay_binary="${relay_binary:-$(command -v "$relay_harness" || true)}"
        printf '%s: %s\n' "$relay_harness" "${relay_binary:-not installed}"
        if [[ -n "$relay_binary" ]]; then
            "$relay_binary" --version || true
            if [[ "$relay_harness" == claude ]]; then "$relay_binary" auth status --text || true
            else "$relay_binary" login status || true; fi
        fi
    done
    gh auth status || true

# Create a persistent local workspace token; preserve existing configuration.
setup:
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -f .env ]]; then
        printf '.env already exists; preserving your configuration.\n'
        exit 0
    fi
    (umask 077; set -o noclobber; printf 'RELAY_TOKEN=%s\n' "${RELAY_TOKEN:-$(openssl rand -hex 32)}" > .env)
    printf 'Created .env with a local workspace token.\n'

# Serve Relay's live GitHub board. Run just client in another terminal.
dogfood repo="jens-hj/relay" owner="jens-hj" board="5":
    #!/usr/bin/env bash
    set -euo pipefail
    if [[ -z "${RELAY_TOKEN:-}" ]]; then
        if [[ -f .env ]]; then
            printf 'Set RELAY_TOKEN in .env or your environment before starting the live server.\n' >&2
            exit 1
        fi
        just setup
        exec just dogfood "$1" "$2" "$3"
    fi
    export RELAY_GITHUB_REPO="$1"
    export RELAY_GITHUB_PROJECT_OWNER="$2"
    export RELAY_GITHUB_PROJECT_NUMBER="$3"
    export RELAY_REPO_PATH="${RELAY_REPO_PATH:-$PWD}"
    export RELAY_DATABASE="${RELAY_DATABASE:-data/dogfood.sqlite3}"
    exec cargo run --locked -p relay-server

# Open the persistent local workspace with installed agent harnesses.
dev *args:
    #!/usr/bin/env bash
    set -euo pipefail
    umask 077

    relay_port="7331"
    relay_release=false
    relay_port_set=false
    while (($#)); do
        case "$1" in
            --release)
                relay_release=true
                ;;
            --*)
                printf 'Unknown option: %s\n' "$1" >&2
                exit 2
                ;;
            *)
                if [[ "$relay_port_set" == true ]]; then
                    printf 'Usage: just dev [--release] [PORT]\n' >&2
                    exit 2
                fi
                relay_port="$1"
                relay_port_set=true
                ;;
        esac
        shift
    done

    if [[ -z "${RELAY_TOKEN:-}" ]]; then
        if [[ -f .env ]]; then printf 'Set RELAY_TOKEN in .env before starting Relay.\n' >&2; exit 1; fi
        just setup
        if [[ "$relay_release" == true ]]; then
            exec just dev --release "$relay_port"
        fi
        exec just dev "$relay_port"
    fi
    export RELAY_DATABASE="${RELAY_DATABASE:-data/local.sqlite3}"
    if ! [[ "$relay_port" =~ ^[0-9]{1,5}$ ]] || ((10#$relay_port < 1 || 10#$relay_port > 65535)); then
        printf 'Port must be a number from 1 to 65535.\n' >&2
        exit 1
    fi

    relay_profile=debug
    relay_cargo_release=()
    if [[ "$relay_release" == true ]]; then
        relay_profile=release
        relay_cargo_release=(--release)
    fi
    cargo build "${relay_cargo_release[@]}" --locked --workspace
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

    "$relay_target_dir/$relay_profile/relay-server" &
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

    "$relay_target_dir/$relay_profile/relay-desktop" &
    relay_client_pid=$!
    relay_status=0
    wait "$relay_client_pid" || relay_status=$?
    relay_client_pid=""
    exit "$relay_status"

# Open isolated fixture-only previews; no real harness turns.
demo port="7331":
    #!/usr/bin/env bash
    set -euo pipefail
    relay_demo_dir="$(mktemp -d)"
    trap 'rm -rf "$relay_demo_dir"' EXIT
    unset RELAY_GITHUB_REPO RELAY_GITHUB_PROJECT_OWNER RELAY_GITHUB_PROJECT_NUMBER RELAY_REPO_PATH
    RELAY_DEMO=1 RELAY_DATABASE="$relay_demo_dir/demo.sqlite3" just dev "$1"

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
