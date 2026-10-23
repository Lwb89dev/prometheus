#!/bin/sh
# Start Prometeus from this tree.
# Builds the daemon, the command line, and the window, stops a daemon that
# already owns the session bus, and opens the window. The window starts the
# daemon from target/debug when the bus name is free. Extra arguments are
# passed to lda-gtk (--config and --data-dir apply only to a daemon this
# window starts). Closing the window leaves the daemon running.
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")" && pwd)
cd "$root"

if [ -z "${DBUS_SESSION_BUS_ADDRESS:-}" ]; then
    echo "lda: no session bus. Run this from the desktop session." >&2
    exit 1
fi
if ! command -v dbus-send >/dev/null 2>&1; then
    echo "lda: dbus-send is not on PATH." >&2
    exit 1
fi

bus_owned() {
    dbus-send --session --dest=org.freedesktop.DBus --type=method_call \
        --print-reply /org/freedesktop/DBus org.freedesktop.DBus.ListNames \
        2>/dev/null | grep -q 'org.linuxdesktopagent.Daemon'
}

ollama_answers() {
    if command -v curl >/dev/null 2>&1; then
        curl -sf --max-time 2 http://127.0.0.1:11434/api/version >/dev/null 2>&1
        return $?
    fi
    if command -v timeout >/dev/null 2>&1; then
        timeout 3 ollama list >/dev/null 2>&1
        return $?
    fi
    ollama list >/dev/null 2>&1
}

ensure_ollama() {
    if ! command -v ollama >/dev/null 2>&1; then
        echo "lda: ollama is not on PATH. The window still opens." >&2
        return 0
    fi
    if ollama_answers; then
        return 0
    fi
    echo "lda: Ollama is not answering on 127.0.0.1:11434. Starting ollama serve."
    mkdir -p "$root/target"
    # A new session so closing the window or the terminal does not kill Ollama.
    if command -v setsid >/dev/null 2>&1; then
        setsid ollama serve >"$root/target/ollama-serve.log" 2>&1 </dev/null &
    else
        nohup ollama serve >"$root/target/ollama-serve.log" 2>&1 &
    fi
    i=0
    while [ "$i" -lt 50 ]; do
        if ollama_answers; then
            return 0
        fi
        i=$((i + 1))
        sleep 0.2
    done
    echo "lda: Ollama did not answer. See target/ollama-serve.log." >&2
}

echo "lda: building lda-daemon, lda-cli, and lda-gtk"
cargo build -p lda-daemon -p lda-cli -p lda-gtk

cli="$root/target/debug/lda-cli"
gtk="$root/target/debug/lda-gtk"

if bus_owned; then
    echo "lda: stopping the daemon already on the session bus"
    "$cli" shutdown
    i=0
    while bus_owned; do
        if [ "$i" -ge 40 ]; then
            echo "lda: the bus name is still owned." >&2
            exit 1
        fi
        i=$((i + 1))
        sleep 0.1
    done
fi

ensure_ollama

echo "lda: opening the window. Closing it leaves the daemon running."
echo "lda: stop that daemon from the window, or with target/debug/lda-cli shutdown"
exec "$gtk" "$@"
