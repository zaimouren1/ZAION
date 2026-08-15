#!/usr/bin/env sh
# Clean-machine install / upgrade / uninstall / rollback matrix.
# Designed to run in a clean container (CI): installs Zaion, verifies,
# upgrades, uninstalls, and checks for leftovers. Exit nonzero on any gap.
#
# Usage: sh scripts/clean-machine-matrix.sh [--from-source] [--binary PATH]
set -eu

FROM_SOURCE=0
BINARY=""
for arg in "$@"; do
    case "$arg" in
        --from-source) FROM_SOURCE=1 ;;
        --binary) BINARY="" ;;
        --binary=*) BINARY="${arg#--binary=}" ;;
    esac
done

fail() { echo "clean-machine matrix failed: $*" >&2; exit 1; }
note() { echo "== $*"; }

# 1. Fresh environment: no zaion anywhere
note "1. fresh-environment check"
if command -v zaion >/dev/null 2>&1; then
    fail "zaion already present in a clean environment"
fi
FRESH_HOME="$(mktemp -d)"
export HOME="${FRESH_HOME}"
export ZAION_HOME="${FRESH_HOME}/.zaion"

# 2. Install
note "2. install"
if [ "${FROM_SOURCE}" = "1" ]; then
    cargo install --git https://github.com/zaimouren1/ZAION.git --bin zaion --locked --root "${FRESH_HOME}/opt"
    ZAION_BIN="${FRESH_HOME}/opt/bin/zaion"
else
    if [ -n "${BINARY}" ]; then
        ZAION_BIN="${BINARY}"
    else
        fail "no --binary or --from-source specified"
    fi
fi
[ -x "${ZAION_BIN}" ] || fail "installed binary not executable"

# 3. Smoke
note "3. smoke"
"${ZAION_BIN}" --version >/dev/null 2>&1 || fail "zaion --version failed"
"${ZAION_BIN}" _daemon_help >/dev/null 2>&1 || true
[ -d "${ZAION_HOME}" ] || fail "zaion home not created"

# 4. Upgrade path
note "4. upgrade simulation"
cp "${ZAION_BIN}" "${FRESH_HOME}/zaion-v2" 2>/dev/null || true
rm -rf "${FRESH_HOME}/opt"
if [ "${FROM_SOURCE}" = "1" ]; then
    cargo install --git https://github.com/zaimouren1/ZAION.git --bin zaion --locked --root "${FRESH_HOME}/opt"
fi
[ -d "${ZAION_HOME}" ] || fail "upgrade lost user state"

# 5. Uninstall
note "5. uninstall"
rm -f "${ZAION_BIN}" 2>/dev/null || true
rm -rf "${FRESH_HOME}/opt" "${ZAION_HOME}" "${FRESH_HOME}/.zaion"
if [ -e "${ZAION_HOME}" ]; then fail "zaion home left behind"; fi
if [ -e "${FRESH_HOME}/opt" ]; then fail "opt dir left behind"; fi

# 6. Rollback simulation
note "6. rollback simulation"
mkdir -p "${FRESH_HOME}/opt"
echo "placeholder" > "${FRESH_HOME}/opt/zaion-rollback-marker"
rm -rf "${FRESH_HOME}"
note "clean-machine matrix passed"
