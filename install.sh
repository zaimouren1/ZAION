#!/usr/bin/env sh
set -eu

REPO="${ZAION_REPO:-zaimouren1/ZAION}"
BINARY="zaion"

OS=""
ARCH=""
TARGET=""
TAG=""
ARCHIVE_EXT=""
ARCHIVE_NAME=""

main() {
    detect_platform
    detect_architecture
    TARGET="$(get_target)"
    ARCHIVE_EXT="$(get_archive_extension)"
    TAG="$(resolve_release_tag)"
    if [ -z "${TAG}" ]; then
        if [ "${DRY_RUN:-}" = "1" ]; then
            echo "[dry-run] No GitHub release found for ${REPO}; would install from source"
            echo "[dry-run] Source URL: https://github.com/${REPO}.git"
            echo "[dry-run] Command: cargo install --git https://github.com/${REPO}.git --bin zaion --locked --force"
            exit 0
        fi
        echo "No GitHub release found for ${REPO}; falling back to source install."
        install_from_source
        verify_installation
        print_next_steps
        exit 0
    fi
    ARCHIVE_NAME="${BINARY}-${TAG}-${TARGET}.${ARCHIVE_EXT}"

    if [ "${DRY_RUN:-}" = "1" ]; then
        echo "[dry-run] Would install ${BINARY} for ${OS}/${ARCH}"
        echo "[dry-run] Release tag: ${TAG}"
        echo "[dry-run] Download URL: $(get_download_url)"
        echo "[dry-run] Checksum URL: $(get_checksum_url)"
        echo "[dry-run] Install path: $(get_install_path)"
        exit 0
    fi

    echo "Installing Zaion..."
    echo "  OS:      ${OS}"
    echo "  Arch:    ${ARCH}"
    echo "  Target:  ${TARGET}"
    echo "  Release: ${TAG}"

    download_and_install
    verify_installation
    print_next_steps
}

detect_platform() {
    OS="$(uname -s)"
    case "${OS}" in
        Linux*) OS=linux ;;
        Darwin*) OS=macos ;;
        CYGWIN*|MINGW*|MSYS*) OS=windows ;;
        *) error "Unsupported OS: ${OS}" ;;
    esac
}

detect_architecture() {
    ARCH="$(uname -m)"
    case "${ARCH}" in
        x86_64|amd64) ARCH=x86_64 ;;
        aarch64|arm64) ARCH=aarch64 ;;
        *) error "Unsupported architecture: ${ARCH}" ;;
    esac
}

get_target() {
    case "${OS}-${ARCH}" in
        linux-x86_64) echo "x86_64-unknown-linux-gnu" ;;
        macos-x86_64) echo "x86_64-apple-darwin" ;;
        macos-aarch64) echo "aarch64-apple-darwin" ;;
        windows-x86_64) echo "x86_64-pc-windows-msvc" ;;
        *) unsupported_binary ;;
    esac
}

unsupported_binary() {
    echo "Error: No prebuilt Zaion release asset for ${OS}/${ARCH}." >&2
    echo "Available prebuilt targets: linux/x86_64, macos/x86_64, macos/aarch64, windows/x86_64." >&2
    echo "You can still build from source with: cargo install --path crates/zaion-cli" >&2
    exit 1
}

get_archive_extension() {
    if [ "${OS}" = "windows" ]; then
        echo "zip"
    else
        echo "tar.gz"
    fi
}

resolve_release_tag() {
    if [ -n "${ZAION_VERSION:-}" ]; then
        case "${ZAION_VERSION}" in
            v*) echo "${ZAION_VERSION}" ;;
            *) echo "v${ZAION_VERSION}" ;;
        esac
        return
    fi

    require_command curl "curl is required to query the latest Zaion release."
    TAG_FROM_API="$(curl -sSf "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
        | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
        | head -n 1 || true)"
    if [ -z "${TAG_FROM_API}" ]; then
        if [ "${DRY_RUN:-}" != "1" ]; then
            echo "No latest release found for ${REPO}; source install fallback will be used." >&2
        fi
        echo ""
        return
    fi
    echo "${TAG_FROM_API}"
}

get_download_url() {
    echo "https://github.com/${REPO}/releases/download/${TAG}/${ARCHIVE_NAME}"
}

get_checksum_url() {
    echo "$(get_download_url).sha256"
}

get_install_path() {
    case "${OS}" in
        linux|macos) echo "/usr/local/bin/zaion" ;;
        windows) echo "C:\\Program Files\\Zaion\\zaion.exe" ;;
    esac
}

download_and_install() {
    require_command curl "curl is required to download Zaion release assets."
    TMPDIR="$(mktemp -d)"
    trap 'rm -rf "${TMPDIR}"' EXIT

    ARCHIVE_PATH="${TMPDIR}/${ARCHIVE_NAME}"
    CHECKSUM_PATH="${ARCHIVE_PATH}.sha256"

    download_asset "$(get_download_url)" "${ARCHIVE_PATH}" "release archive"
    download_asset "$(get_checksum_url)" "${CHECKSUM_PATH}" "checksum file"
    verify_checksum "${ARCHIVE_PATH}" "${CHECKSUM_PATH}"

    case "${OS}" in
        linux|macos) install_unix "${ARCHIVE_PATH}" "${TMPDIR}" ;;
        windows) install_windows "${ARCHIVE_PATH}" "${TMPDIR}" ;;
    esac

    echo "Installed zaion to $(get_install_path)"
}

install_from_source() {
    require_command cargo "cargo is required for source fallback install. Install Rust from https://rustup.rs or set ZAION_VERSION to a release tag."
    require_command git "git is required for source fallback install."
    echo "Security notice: source fallback is not a checksum-verified binary release." >&2
    echo "Review the selected repository and default branch before continuing." >&2
    echo "Installing Zaion from source with cargo..."
    cargo install --git "https://github.com/${REPO}.git" --bin zaion --locked --force
    echo "Installed zaion through cargo install."
    echo "Cargo bin path: ${CARGO_HOME:-$HOME/.cargo}/bin"
}

download_asset() {
    URL="$1"
    DEST="$2"
    LABEL="$3"

    echo "Downloading ${LABEL}: ${URL}"
    if ! curl -sSfL "${URL}" -o "${DEST}"; then
        echo "Error: Could not download ${LABEL}." >&2
        echo "Missing URL: ${URL}" >&2
        echo "The ${TAG} release may not include ${TARGET}. Check https://github.com/${REPO}/releases/tag/${TAG}" >&2
        exit 1
    fi
}

verify_checksum() {
    ARCHIVE_PATH="$1"
    CHECKSUM_PATH="$2"

    NONEMPTY_LINES="$(awk 'NF { count++ } END { print count + 0 }' "${CHECKSUM_PATH}")"
    if [ "${NONEMPTY_LINES}" -ne 1 ]; then
        error "Checksum file must contain exactly one non-empty record: ${CHECKSUM_PATH}"
    fi

    CHECKSUM_LINE="$(sed -n '1{s/\r$//;p;}' "${CHECKSUM_PATH}")"
    FIELD_COUNT="$(printf '%s\n' "${CHECKSUM_LINE}" | awk '{ print NF }')"
    EXPECTED="$(printf '%s\n' "${CHECKSUM_LINE}" | awk '{ print $1 }')"
    CHECKSUM_ARCHIVE="$(printf '%s\n' "${CHECKSUM_LINE}" | awk '{ print $2 }')"
    if [ "${FIELD_COUNT}" -ne 2 ] || [ "${#EXPECTED}" -ne 64 ]; then
        error "Checksum file must use '<64-char-sha256>  <archive-name>': ${CHECKSUM_PATH}"
    fi
    case "${EXPECTED}" in
        *[!A-Fa-f0-9]*) error "Checksum file contains a non-hex SHA-256 digest: ${CHECKSUM_PATH}" ;;
        0000000000000000000000000000000000000000000000000000000000000000)
            error "Checksum file contains a placeholder SHA-256 digest: ${CHECKSUM_PATH}"
            ;;
    esac
    if [ "${CHECKSUM_ARCHIVE}" != "${ARCHIVE_NAME}" ]; then
        error "Checksum archive name mismatch: expected ${ARCHIVE_NAME}, found ${CHECKSUM_ARCHIVE}"
    fi
    EXPECTED="$(printf '%s' "${EXPECTED}" | tr 'A-F' 'a-f')"

    if command -v sha256sum >/dev/null 2>&1; then
        ACTUAL="$(sha256sum "${ARCHIVE_PATH}" | awk '{print $1}')"
    elif command -v shasum >/dev/null 2>&1; then
        ACTUAL="$(shasum -a 256 "${ARCHIVE_PATH}" | awk '{print $1}')"
    elif command -v powershell.exe >/dev/null 2>&1; then
        ACTUAL="$(powershell.exe -NoProfile -Command "(Get-FileHash -Algorithm SHA256 -LiteralPath '${ARCHIVE_PATH}').Hash.ToLowerInvariant()" | tr -d '\r')"
    else
        error "No SHA-256 tool found. Install sha256sum, shasum, or PowerShell and retry."
    fi
    ACTUAL="$(printf '%s' "${ACTUAL}" | tr 'A-F' 'a-f' | tr -d '\r\n')"

    if [ "${EXPECTED}" != "${ACTUAL}" ]; then
        echo "Error: Checksum verification failed for ${ARCHIVE_NAME}." >&2
        echo "Expected: ${EXPECTED}" >&2
        echo "Actual:   ${ACTUAL}" >&2
        exit 1
    fi

    echo "Checksum verified: ${ARCHIVE_NAME}"
}

install_unix() {
    ARCHIVE_PATH="$1"
    TMPDIR="$2"
    INSTALL_DIR="/usr/local/bin"

    require_command tar "tar is required to extract Zaion release archives."
    tar xzf "${ARCHIVE_PATH}" -C "${TMPDIR}"
    if [ ! -f "${TMPDIR}/zaion" ]; then
        error "Release archive did not contain the zaion binary."
    fi

    if [ -w "${INSTALL_DIR}" ]; then
        cp "${TMPDIR}/zaion" "${INSTALL_DIR}/zaion"
        chmod +x "${INSTALL_DIR}/zaion"
    else
        require_command sudo "sudo is required to install to ${INSTALL_DIR}."
        echo "Requires sudo to install to ${INSTALL_DIR}"
        sudo cp "${TMPDIR}/zaion" "${INSTALL_DIR}/zaion"
        sudo chmod +x "${INSTALL_DIR}/zaion"
    fi
}

install_windows() {
    ARCHIVE_PATH="$1"
    TMPDIR="$2"
    INSTALL_DIR="/c/Program Files/Zaion"

    require_command unzip "unzip is required to extract Zaion Windows release archives."
    mkdir -p "${INSTALL_DIR}"
    unzip -q -o "${ARCHIVE_PATH}" -d "${TMPDIR}"
    if [ ! -f "${TMPDIR}/zaion.exe" ]; then
        error "Release archive did not contain zaion.exe."
    fi
    cp "${TMPDIR}/zaion.exe" "${INSTALL_DIR}/zaion.exe"

    if command -v powershell.exe >/dev/null 2>&1; then
        powershell.exe -NoProfile -Command "\$dir = 'C:\Program Files\Zaion'; \$userPath = [Environment]::GetEnvironmentVariable('Path', 'User'); if (-not ((\$userPath -split ';') -contains \$dir)) { \$newPath = ((\$userPath, \$dir) | Where-Object { \$_ }) -join ';'; [Environment]::SetEnvironmentVariable('Path', \$newPath, 'User') }"
        echo "Added C:\\Program Files\\Zaion to the user PATH when it was not already present."
    else
        echo "Note: powershell.exe was not found, so PATH was not updated automatically."
    fi
}

verify_installation() {
    if command -v zaion >/dev/null 2>&1; then
        echo "Verification: $(zaion --version 2>/dev/null || echo 'installed')"
    else
        echo "Verification: zaion was installed, but this shell cannot find it on PATH yet."
    fi
    print_path_note
}

print_path_note() {
    case "${OS}" in
        windows)
            echo "PATH note for Windows:"
            echo "  Close and reopen PowerShell, Windows Terminal, Git Bash, VS Code, or your IDE terminal."
            echo "  New shells inherit the updated user PATH. This current shell may not."
            echo "  Temporary Git Bash PATH for this shell: export PATH=\"\$PATH:/c/Program Files/Zaion\""
            ;;
        linux|macos)
            echo "PATH note:"
            echo "  Zaion installs to /usr/local/bin. Open a new shell if your current shell cached PATH."
            echo "  If /usr/local/bin is missing, add this to your shell profile: export PATH=\"/usr/local/bin:\$PATH\""
            ;;
    esac
}

print_next_steps() {
    echo ""
    echo "Next steps:"
    echo "  zaion onboard"
    echo "  zaion doctor"
    echo "  zaion chat \"Hello\""
    echo "  zaion tui"
    echo ""
    echo "No interactive onboarding was started automatically."
}

require_command() {
    NAME="$1"
    MESSAGE="$2"
    if ! command -v "${NAME}" >/dev/null 2>&1; then
        error "${MESSAGE}"
    fi
}

error() {
    echo "Error: $*" >&2
    exit 1
}

main
