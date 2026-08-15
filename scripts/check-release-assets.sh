#!/usr/bin/env sh
set -eu

ROOT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
cd "${ROOT_DIR}"

fail() {
    echo "release validation failed: $*" >&2
    exit 1
}

require_file() {
    if [ ! -f "$1" ]; then
        fail "missing required file: $1"
    fi
}

require_text() {
    FILE="$1"
    NEEDLE="$2"
    LABEL="$3"
    if ! grep -F -- "${NEEDLE}" "${FILE}" >/dev/null 2>&1; then
        fail "${LABEL} missing from ${FILE}: ${NEEDLE}"
    fi
}

require_absent() {
    FILE="$1"
    NEEDLE="$2"
    LABEL="$3"
    if grep -F -- "${NEEDLE}" "${FILE}" >/dev/null 2>&1; then
        fail "${LABEL} must not appear in ${FILE}: ${NEEDLE}"
    fi
}

require_final_docker_user() {
    EXPECTED="$1"
    FINAL_USER="$(awk 'toupper($1) == "USER" { user = $2 } END { print user }' Dockerfile)"
    if [ "${FINAL_USER}" != "${EXPECTED}" ]; then
        fail "final Docker user must be ${EXPECTED}, found ${FINAL_USER:-<none>}"
    fi
}

package_templates_have_placeholders() {
    grep -F "PLACEHOLDER_SHA256" homebrew-formula.rb winget-manifest.yaml >/dev/null 2>&1
}

require_file ".github/workflows/ci.yml"
require_file ".github/workflows/docker.yml"
require_file "LICENSE"
require_file "install.sh"
require_file "install.ps1"
require_file "homebrew-formula.rb"
require_file "winget-manifest.yaml"
require_file "Dockerfile"
require_file "zaion.service"
require_file "crates/zaion-cli/src/commands/mod.rs"
require_file "docs/RELEASE.md"

require_text "LICENSE" "Apache License" "Repository license"

require_text ".github/workflows/ci.yml" "cargo check --workspace --all-targets --locked" "Locked Rust all-targets check"
require_text ".github/workflows/ci.yml" "toolchain: 1.93.0" "Pinned Rust CI toolchain"
require_file "rust-toolchain.toml"
require_text "rust-toolchain.toml" 'channel = "1.93.0"' "Pinned repository Rust toolchain"
require_text "Cargo.toml" 'rust-version = "1.93"' "Declared workspace MSRV"
require_text ".github/workflows/ci.yml" 'branches: ["**"]' "Slash-containing branch coverage"
require_text ".github/workflows/ci.yml" "cargo test --workspace --locked -j1 -- --test-threads=1" "Locked serial workspace tests"
require_text ".github/workflows/ci.yml" "fresh-home CLI smoke tests" "Fresh-home smoke coverage"
require_text ".github/workflows/ci.yml" "architecture-audit --root ." "Explicit source architecture audit"
require_text ".github/workflows/ci.yml" "cargo clippy --workspace --all-targets --locked -- -D warnings" "Locked Clippy all-targets gate"
require_absent ".github/workflows/ci.yml" "zaion-website" "Retired public website CI"
require_absent ".github/workflows/ci.yml" "npm run lint" "Retired public website lint gate"
require_absent ".github/workflows/ci.yml" "npm run build" "Retired public website build gate"
require_text ".github/workflows/ci.yml" "cargo audit" "Scheduled cargo audit"
require_text ".github/workflows/ci.yml" "scripts/check-release-assets.sh" "Release validation job"
require_text ".github/workflows/ci.yml" ".sha256" "Checksum asset generation"
require_text ".github/workflows/ci.yml" "if: startsWith(github.ref, 'refs/tags/v')" "Tag-only release gate"
require_absent ".github/workflows/ci.yml" "homebrew-formula.rb" "Placeholder Homebrew template release upload"
require_absent ".github/workflows/ci.yml" "winget-manifest.yaml" "Placeholder Winget template release upload"

for target in \
    x86_64-unknown-linux-gnu \
    x86_64-apple-darwin \
    aarch64-apple-darwin \
    x86_64-pc-windows-msvc
do
    require_text ".github/workflows/ci.yml" "target: ${target}" "Release target ${target}"
done

require_text "install.sh" "x86_64-unknown-linux-gnu" "Installer Linux target"
require_text "install.sh" "x86_64-apple-darwin" "Installer macOS Intel target"
require_text "install.sh" "aarch64-apple-darwin" "Installer macOS Apple Silicon target"
require_text "install.sh" "x86_64-pc-windows-msvc" "Installer Windows target"
require_text "install.sh" "verify_checksum" "Installer checksum verification"
require_text "install.sh" ".sha256" "Installer checksum download"
require_text "install.sh" "Checksum archive name mismatch" "Installer checksum filename binding"
require_text "install.sh" "not a checksum-verified binary release" "Installer unsigned source fallback notice"
require_absent "install.sh" "Signature verified" "Installer false signature claim"
require_text "install.sh" "source install fallback" "Installer source fallback"
require_text "install.sh" "cargo install --git" "Installer cargo source install"
require_text "install.sh" "No prebuilt Zaion release asset" "Installer missing asset error"
require_text "install.sh" "Close and reopen PowerShell" "Windows PATH restart guidance"
require_text "install.sh" "No interactive onboarding was started automatically." "Non-interactive install completion"
require_absent "install.sh" "run_onboard" "Automatic onboarding"
require_absent "install.sh" "zaion onboard ||" "Automatic onboarding command"

require_text "install.ps1" "x86_64-pc-windows-msvc" "PowerShell installer Windows target"
require_text "install.ps1" ".sha256" "PowerShell installer checksum download"
require_text "install.ps1" "Get-FileHash -Algorithm SHA256" "PowerShell installer checksum verification"
require_text "install.ps1" "Checksum archive name mismatch" "PowerShell checksum filename binding"
require_text "install.ps1" "not a checksum-verified binary release" "PowerShell unsigned source fallback notice"
require_absent "install.ps1" "Signature verified" "PowerShell false signature claim"
require_text "install.ps1" "source install fallback" "PowerShell source fallback"
require_text "install.ps1" "cargo install --git" "PowerShell cargo source install"
require_text "install.ps1" "zaion.exe" "PowerShell installer single binary"
require_text "install.ps1" "No interactive onboarding was started automatically." "PowerShell non-interactive install completion"
require_absent "install.ps1" "zaion-tui" "PowerShell installer separate TUI binary"

require_text "homebrew-formula.rb" "Release template" "Homebrew template marker"
require_text "homebrew-formula.rb" "x86_64-apple-darwin.tar.gz" "Homebrew macOS Intel asset"
require_text "homebrew-formula.rb" "aarch64-apple-darwin.tar.gz" "Homebrew macOS Apple Silicon asset"
require_text "homebrew-formula.rb" "x86_64-unknown-linux-gnu.tar.gz" "Homebrew Linux asset"
require_text "homebrew-formula.rb" 'run [opt_bin/"zaion", "_daemon_run"]' "Homebrew foreground runtime service"

require_text "Dockerfile" 'CMD ["_daemon_run"]' "Container foreground runtime"
require_text "Dockerfile" 'ENTRYPOINT ["zaion"]' "Container executable entrypoint"
require_text "Dockerfile" "FROM rust:1.93-slim AS builder" "Container Rust toolchain"
require_text "Dockerfile" "HOME=/home/zaion" "Container explicit home"
require_text "Dockerfile" "ZAION_HOME=/var/lib/zaion" "Container explicit Zaion home"
require_text "Dockerfile" "ZAION_DATA_DIR=/var/lib/zaion/data" "Container explicit writable data directory"
require_text "Dockerfile" 'VOLUME ["/var/lib/zaion"]' "Container persistent state volume"
require_text "Dockerfile" "ZAION_GATEWAY_BIND=0.0.0.0:7821" "Container explicit external gateway bind"
require_text "Dockerfile" "EXPOSE 7821" "Container gateway port"
require_text "Dockerfile" "HEALTHCHECK" "Container health check"
require_text "Dockerfile" "gateway health: verified" "Container strong-identity health check"
require_final_docker_user "10001:10001"
require_absent "Dockerfile" "EXPOSE 9753" "Retired container port"
require_text "zaion.service" "ExecStart=/usr/local/bin/zaion _daemon_run" "systemd foreground runtime"
require_text "crates/zaion-cli/src/commands/mod.rs" '"_daemon_run" => network::cmd_daemon_run(args)' "Foreground runtime command registration"
require_absent "Dockerfile" 'CMD ["singularity", "start", "--daemon"]' "Experimental container startup"
require_absent "zaion.service" "singularity start --daemon" "Experimental systemd startup"

require_text "winget-manifest.yaml" "Release template" "Winget template marker"
require_text "winget-manifest.yaml" "x86_64-pc-windows-msvc.zip" "Winget Windows asset"
require_text "docs/RELEASE.md" "Replace checksum placeholders" "Release checksum publishing docs"
require_text "docs/RELEASE.md" "not a cryptographic release signature" "Release checksum authenticity boundary"
require_text "docs/RELEASE.md" "must not be published" "Package template publication boundary"
require_text "docs/RELEASE.md" '127.0.0.1:7821:7821' "Safe container host bind example"
require_text "docs/RELEASE.md" 'ZAION_GATEWAY_TOKEN' "External container bearer requirement"
require_text "docs/RELEASE.md" 'UID/GID `10001:10001`' "Container non-root identity docs"
require_text "docs/RELEASE.md" "GHCR images are not signed" "Unsigned container image boundary"

if package_templates_have_placeholders; then
    if [ "${ZAION_PACKAGE_PUBLISH:-0}" = "1" ]; then
        fail "package templates contain PLACEHOLDER_SHA256 values and must not be published"
    fi
    echo "Package templates: NOT PUBLISHABLE (checksum placeholders remain)."
else
require_file "scripts/gen-sbom.py"
require_file "scripts/sign-artifact.py"
require_text "docs/RELEASE.md" "gen-sbom" "SBOM generation documented"

# SBOM generation must succeed and produce a realistic component count.
SBOM_JSON="$(mktemp)"
if python3 scripts/gen-sbom.py --out "${SBOM_JSON}" >/dev/null 2>&1 \
    || python scripts/gen-sbom.py --out "${SBOM_JSON}" >/dev/null 2>&1; then
    COMPONENT_COUNT="$(grep -o '"component_count": [0-9]*' "${SBOM_JSON}" | grep -o '[0-9]*' || echo 0)"
    rm -f "${SBOM_JSON}"
    if [ "${COMPONENT_COUNT:-0}" -lt 100 ]; then
        fail "SBOM generation produced too few components: ${COMPONENT_COUNT:-0}"
    fi
    echo "SBOM: generated (${COMPONENT_COUNT} components)."
else
    fail "SBOM generation failed"
fi
    echo "Package templates: checksum placeholders cleared."
fi
echo "Release authenticity: UNSIGNED (SHA-256 integrity sidecars only)."
echo "Release asset and install-chain validation passed."
