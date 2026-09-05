#!/usr/bin/env bash
#
# galdeck development helper.
#
#   scripts/dev.sh doctor              check the environment before anything else
#   scripts/dev.sh check               fmt, clippy and tests, the way CI runs them
#   scripts/dev.sh calibrate [args]    run the calibration wizard on the device
#   scripts/dev.sh layout [--json]     print the saved layout, no device needed
#   scripts/dev.sh probe <mode>        raw protocol probes (see --help)
#   scripts/dev.sh daemon <cmd>        start/stop/restart/status the user service
#   scripts/dev.sh bundle [out.tar.gz] collect a diagnostics bundle for a report
#
# Anything that talks to the device stops the daemon first and restarts it
# afterwards, because two open handles on one hidraw node is the leading
# suspect for the module dropping off the USB bus.

set -euo pipefail

REPO="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO"

VENDOR_PRODUCT="1b1c:2b18"
SERVICE="galdeck.service"
LAYOUT="${XDG_CONFIG_HOME:-$HOME/.config}/galdeck/layout.conf"

bold() { printf '\033[1m%s\033[0m\n' "$*"; }
ok()   { printf '  \033[32m✓\033[0m %s\n' "$*"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$*"; }
bad()  { printf '  \033[31m✗\033[0m %s\n' "$*"; }

# ---- daemon handling -------------------------------------------------------

daemon_active() {
    systemctl --user is-active --quiet "$SERVICE" 2>/dev/null
}

# Frees the device for the duration of a command, then puts it back exactly as
# it was. The trap matters: a failed run must not leave the deck dark.
RESTORE_DAEMON=0
release_device() {
    if daemon_active; then
        echo "==> stopping $SERVICE for exclusive device access"
        systemctl --user stop "$SERVICE"
        RESTORE_DAEMON=1
        trap restore_device EXIT INT TERM
    fi
}
restore_device() {
    if [ "$RESTORE_DAEMON" = "1" ]; then
        RESTORE_DAEMON=0
        echo "==> restarting $SERVICE"
        systemctl --user start "$SERVICE" || warn "could not restart $SERVICE"
    fi
}

# ---- subcommands -----------------------------------------------------------

cmd_doctor() {
    bold "toolchain"
    if command -v cargo >/dev/null; then
        ok "$(cargo --version)"
        ok "$(rustc --version)"
    else
        bad "cargo not found"
    fi
    command -v cargo-clippy >/dev/null && ok "clippy available" || warn "clippy missing: rustup component add clippy"

    bold "device"
    if lsusb 2>/dev/null | grep -qi "$VENDOR_PRODUCT"; then
        ok "$(lsusb | grep -i "$VENDOR_PRODUCT")"
    else
        bad "stream deck module $VENDOR_PRODUCT not on the bus"
        lsusb 2>/dev/null | grep -i 1b1c || true
    fi

    bold "permissions"
    local readable=0
    for node in /dev/hidraw*; do
        [ -r "$node" ] && readable=$((readable + 1))
    done
    if [ "$readable" -gt 0 ]; then
        ok "$readable hidraw node(s) readable by $(id -un)"
    else
        bad "no readable hidraw nodes — install the rule:"
        echo "      sudo cp udev/70-galdeck.rules /etc/udev/rules.d/ && sudo udevadm control --reload"
    fi

    bold "daemon"
    if systemctl --user list-unit-files "$SERVICE" >/dev/null 2>&1; then
        if daemon_active; then
            ok "$SERVICE running (stop it before using the device directly)"
        else
            warn "$SERVICE installed but stopped — your deck will be dark"
        fi
    else
        warn "$SERVICE not installed"
    fi

    bold "layout"
    if [ -f "$LAYOUT" ]; then
        ok "$LAYOUT"
        sed -n 's/^\(version\|bounds\|matrix\|bleed\) *=/  &/p' "$LAYOUT" | sed 's/^/    /'
    else
        warn "no saved layout — run: scripts/dev.sh calibrate"
    fi
}

cmd_check() {
    bold "fmt"
    cargo fmt --all -- --check
    bold "clippy"
    cargo clippy --all-targets --all-features -- -D warnings
    bold "test"
    cargo test --all-features
    bold "build (no default features)"
    cargo check --no-default-features
}

cmd_build() { cargo build --examples "$@"; }

cmd_calibrate() {
    release_device
    cargo run --quiet --example calibrate -- "$@"
}

cmd_layout() {
    # No device needed, so no daemon juggling.
    cargo run --quiet --example calibrate -- "${1:---print}"
}

cmd_probe() {
    release_device
    cargo run --quiet --example keycal -- "$@"
}

cmd_verify() {
    release_device
    cargo run --quiet --example verify -- "$@"
}

cmd_daemon() {
    case "${1:-status}" in
        start)   systemctl --user start "$SERVICE" ;;
        stop)    systemctl --user stop "$SERVICE" ;;
        restart) systemctl --user restart "$SERVICE" ;;
        status)  systemctl --user status "$SERVICE" --no-pager || true ;;
        log)     journalctl --user -u "$SERVICE" -n "${2:-100}" --no-pager ;;
        *)       bad "unknown daemon command: $1"; return 1 ;;
    esac
}

cmd_bundle() {
    local out="${1:-galdeck-debug-$(date +%Y%m%d-%H%M%S).tar.gz}"
    local dir
    dir="$(mktemp -d)"
    local stage="$dir/galdeck-debug"
    mkdir -p "$stage"

    echo "==> collecting diagnostics"

    {
        echo "collected: $(date -Is)"
        echo "host:      $(uname -a)"
        echo "user:      $(id -un)"
    } > "$stage/system.txt"

    {
        cargo --version 2>&1 || true
        rustc --version 2>&1 || true
        echo
        echo "--- git ---"
        git -C "$REPO" rev-parse HEAD 2>&1 || true
        git -C "$REPO" status --short 2>&1 || true
    } > "$stage/toolchain.txt"

    lsusb 2>&1 | grep -i 1b1c > "$stage/usb.txt" || echo "no 1b1c devices" > "$stage/usb.txt"
    ls -l /dev/hidraw* > "$stage/hidraw.txt" 2>&1 || true

    # Identity getters open the device read-only and do not enter software
    # mode, so this is safe to run with the daemon up.
    if cargo run --quiet --example detect > "$stage/detect.txt" 2>&1; then
        echo "==> device identified"
    else
        warn "detect failed — see detect.txt"
    fi

    if [ -f "$LAYOUT" ]; then
        cp "$LAYOUT" "$stage/layout.conf"
        cargo run --quiet --example calibrate -- --json > "$stage/layout.json" 2>/dev/null || true
        cargo run --quiet --example calibrate -- --print > "$stage/layout.txt" 2>/dev/null || true
    fi

    journalctl --user -u "$SERVICE" -n 200 --no-pager > "$stage/daemon.log" 2>&1 || true
    # USB resets and disconnects are the first thing to check when the module
    # vanishes mid-session. Match the kernel's own USB/HID lines only —
    # AppArmor audit spam from snap-confined lsusb would otherwise bury them.
    (dmesg 2>/dev/null || journalctl -k -n 2000 --no-pager 2>/dev/null) \
        | grep -vi "apparmor" \
        | grep -iE "usb [0-9]+-|new (full|high|super)-speed usb|device descriptor|disconnect|reset (full|high|super)-speed|hid-generic|hidraw|1b1c" \
        | tail -100 > "$stage/kernel.log" || true
    if [ ! -s "$stage/kernel.log" ]; then
        echo "no usb/hid kernel messages found (dmesg may need root)" > "$stage/kernel.log"
    fi

    cp "$REPO/Cargo.toml" "$stage/" 2>/dev/null || true

    tar -czf "$out" -C "$dir" galdeck-debug
    rm -rf "$dir"
    bold "wrote $out"
    tar -tzf "$out" | sed 's/^/  /'
    echo
    echo "Review it before sharing — it contains your device serial and hostname."
}

usage() {
    sed -n '3,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

case "${1:-doctor}" in
    doctor)    shift; cmd_doctor "$@" ;;
    check)     shift; cmd_check "$@" ;;
    build)     shift; cmd_build "$@" ;;
    calibrate) shift; cmd_calibrate "$@" ;;
    layout)    shift; cmd_layout "$@" ;;
    probe)     shift; cmd_probe "$@" ;;
    verify)    shift; cmd_verify "$@" ;;
    daemon)    shift; cmd_daemon "$@" ;;
    bundle)    shift; cmd_bundle "$@" ;;
    -h|--help|help) usage ;;
    *)         bad "unknown command: $1"; echo; usage; exit 1 ;;
esac
