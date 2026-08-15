# Release Chain

Zaion release tags use `vX.Y.Z`. The CI release job builds the single `zaion`
binary, packages it, generates a SHA-256 sidecar file, and uploads both files
to the GitHub release. Terminal chat is embedded behind `zaion` / `zaion tui`;
`zaion dashboard open` is the browser WebUI. Releases do not require a second
user-facing TUI binary.

## CI Gates

Every push and pull request runs:

- `cargo check --workspace --all-targets --locked`
- `cargo test --workspace --locked -j1 -- --test-threads=1`, including the
  fresh-home CLI smoke tests
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo fmt --all -- --check`
- release asset and install-chain validation through
  `scripts/check-release-assets.sh`

The former standalone public website and its Node.js CI job are retired. The
browser control plane is compiled into the Rust gateway.

Container, systemd, and Homebrew services run `zaion _daemon_run` as the
foreground process. User-facing `zaion start` remains the command that spawns
that daemon in the background for an interactive local installation.

### Container Runtime Boundary

The final container runs as the non-root UID/GID `10001:10001`. It sets
`HOME=/home/zaion`, `ZAION_HOME=/var/lib/zaion`, and
`ZAION_DATA_DIR=/var/lib/zaion/data`; `/var/lib/zaion` is the persistent state
volume and must remain writable by that identity. The image health check calls
the strong gateway identity probe and only accepts
`zaion.gateway.health.v1`. `zaion _daemon_run` remains the foreground PID 1
process so normal container stop signals reach the runtime.

The image binds `0.0.0.0:7821` inside its network namespace so an operator can
publish the gateway deliberately. `EXPOSE` does not publish the port. Prefer a
loopback-only host mapping:

```bash
docker run --rm \
  -p 127.0.0.1:7821:7821 \
  -e ZAION_GATEWAY_TOKEN='<at-least-32-random-bytes>' \
  -v zaion-state:/var/lib/zaion \
  <zaion-image>
```

Because the container binds a non-loopback interface, startup refuses to serve
unless `ZAION_GATEWAY_TOKEN` contains at least 32 bytes. API clients must send
that value as a bearer token. Same-origin browser requests are allowed; any
additional browser origins must be listed explicitly in
`ZAION_GATEWAY_ALLOWED_ORIGINS`. This bearer/origin boundary is a P0 safeguard,
not an enterprise identity layer. Publishing on a LAN or public interface still
requires firewall policy and a TLS reverse proxy, and the container must not be
exposed directly to an untrusted network before RBAC and security review close.

The security audit runs on the weekly schedule and on manual
`workflow_dispatch`:

```bash
cargo audit
```

## Release Assets

The release workflow publishes these archive names:

- `zaion-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz`
- `zaion-vX.Y.Z-x86_64-apple-darwin.tar.gz`
- `zaion-vX.Y.Z-aarch64-apple-darwin.tar.gz`
- `zaion-vX.Y.Z-x86_64-pc-windows-msvc.zip`

Each archive must have a matching sidecar:

```text
zaion-vX.Y.Z-<target>.<ext>.sha256
```

The sidecar format is the standard two-column checksum format:

```text
<64-char-sha256>  zaion-vX.Y.Z-<target>.<ext>
```

Installers require exactly one non-empty sidecar record and require its archive
name to match the downloaded asset. A SHA-256 sidecar detects corruption or an
archive/sidecar mismatch, but it is not a cryptographic release signature: an
attacker able to replace both assets can replace both values. The release
workflow does not currently generate Sigstore, GPG, or another publisher
signature, and GHCR images are not signed. Do not describe binary archives or
container images as signed releases until signature generation and installer
verification are both implemented.

## Installer Behavior

One-command install:

```bash
curl -fsSL https://raw.githubusercontent.com/zaimouren1/ZAION/main/install.sh | sh
```

```powershell
irm https://raw.githubusercontent.com/zaimouren1/ZAION/main/install.ps1 | iex
```

`install.sh` and `install.ps1` first try the release archive and its `.sha256`
sidecar, verify the SHA-256 digest locally, and fail with a clear missing-asset
URL when a tagged release is selected but the asset is unavailable.

If the repository has no latest GitHub release yet, both installers use a
source install fallback:

```bash
cargo install --git https://github.com/zaimouren1/ZAION.git --bin zaion --locked --force
```

That keeps the public one-command install usable during the first GitHub launch
before release assets exist. The fallback requires Rust/Cargo and Git; tagged
release installs remain the preferred path for normal users. Installers print a
security notice because source fallback is not a checksum-verified binary
release and builds the selected repository's default branch.

Publication blocker: the default `zaimouren1/ZAION` repository must be
publicly reachable, or `ZAION_REPO` must be changed before publishing the
installer. If the repository is private or missing, the source fallback cannot
complete.


## Artifact Signing

Artifacts may be signed with an Ed25519 key (the project does not commit keys):

```sh
python scripts/sign-artifact.py gen-key --key release-ed25519.pem --pub release-ed25519.pub.pem
python scripts/sign-artifact.py sign --key release-ed25519.pem --in zaion-v0.1.0.tar.gz --out zaion-v0.1.0.tar.gz.sig
python scripts/sign-artifact.py verify --pub release-ed25519.pub.pem --in zaion-v0.1.0.tar.gz --sig zaion-v0.1.0.tar.gz.sig
```

Until a release key is provisioned, releases remain integrity-verified via SHA-256 sidecars only (no cryptographic signature claims are made).


## SBOM

Every release must include a software bill of materials generated from the locked
dependency graph:

```sh
python scripts/gen-sbom.py            # -> target/sbom.json (CycloneDX 1.5)
```

The SBOM lists every crate in `Cargo.lock` (name, version, source) and is
regenerated at release time. `scripts/check-release-assets.sh` fails the release
if the SBOM cannot be generated or contains too few components.

The installer does not start interactive onboarding after install. It prints
the next commands instead:

```bash
zaion onboard
zaion doctor
zaion chat "Hello"
zaion tui
```

On Windows, the installer updates the user PATH when PowerShell is available,
then tells the user to close and reopen PowerShell, Windows Terminal, Git Bash,
VS Code, or the IDE terminal so the new PATH is inherited.

## Package Templates

`homebrew-formula.rb` and `winget-manifest.yaml` are release templates.
Replace checksum placeholders with the digest from the generated sidecar before
publishing either package:

- Homebrew Intel macOS: `PLACEHOLDER_SHA256_INTEL`
- Homebrew Apple Silicon macOS: `PLACEHOLDER_SHA256_ARM`
- Homebrew Linux x86_64: `PLACEHOLDER_SHA256_LINUX`
- Winget Windows x64: `PLACEHOLDER_SHA256_WINDOWS_X64`

Replace checksum placeholders only after the tagged GitHub release has uploaded
all archives and `.sha256` files. Files that still contain any
`PLACEHOLDER_SHA256_*` value must not be published or uploaded as release
assets; the release validation gate enforces that the templates are not part of
the automated GitHub release upload. Before publishing either package template,
run the strict package gate:

```bash
ZAION_PACKAGE_PUBLISH=1 scripts/check-release-assets.sh
```

The normal CI invocation reports placeholder-bearing templates as
`NOT PUBLISHABLE` without failing because those templates are not GitHub release
assets. Strict package mode fails until every placeholder is replaced.
