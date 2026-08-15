$ErrorActionPreference = "Stop"

$Repo = if ($env:ZAION_REPO) { $env:ZAION_REPO } else { "zaimouren1/ZAION" }
$Version = $env:ZAION_VERSION
$InstallDir = if ($env:ZAION_INSTALL_DIR) {
    $env:ZAION_INSTALL_DIR
} else {
    Join-Path $env:LOCALAPPDATA "Programs\Zaion\bin"
}

function Fail($Message) {
    Write-Error $Message
    exit 1
}

function Resolve-ReleaseTag {
    if ($Version) {
        if ($Version.StartsWith("v")) { return $Version }
        return "v$Version"
    }

    $ApiUrl = "https://api.github.com/repos/$Repo/releases/latest"
    try {
        return (Invoke-RestMethod -Uri $ApiUrl -Headers @{ "User-Agent" = "zaion-installer" }).tag_name
    } catch {
        if ($env:DRY_RUN -ne "1") {
            Write-Warning "No latest release found for $Repo; source install fallback will be used."
        }
        return $null
    }
}

function Install-FromSource {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Fail "cargo is required for source fallback install. Install Rust from https://rustup.rs or set ZAION_VERSION to a release tag."
    }
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
        Fail "git is required for source fallback install."
    }

    $SourceUrl = "https://github.com/$Repo.git"
    Write-Warning "Source fallback is not a checksum-verified binary release. Review the selected repository and default branch before continuing."
    Write-Host "Installing Zaion from source with cargo..."
    Write-Host "  Source: $SourceUrl"
    cargo install --git $SourceUrl --bin zaion --locked --force
    if ($LASTEXITCODE -ne 0) {
        Fail "cargo install source fallback failed."
    }
    Write-Host "Installed zaion through cargo install."
}

function Add-ToUserPath($Dir) {
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    $Parts = @()
    if ($UserPath) {
        $Parts = $UserPath -split ";" | Where-Object { $_ }
    }
    if ($Parts -notcontains $Dir) {
        $NewPath = (@($Parts) + $Dir) -join ";"
        [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
    }
    if (($env:Path -split ";") -notcontains $Dir) {
        $env:Path = "$env:Path;$Dir"
    }
}

function Download-File($Url, $Destination, $Label) {
    Write-Host "Downloading ${Label}: $Url"
    try {
        Invoke-WebRequest -Uri $Url -OutFile $Destination -Headers @{ "User-Agent" = "zaion-installer" }
    } catch {
        Fail "Could not download $Label. Missing URL: $Url"
    }
}

if (-not [Environment]::Is64BitOperatingSystem) {
    Fail "No prebuilt Zaion release asset for 32-bit Windows."
}

$Target = "x86_64-pc-windows-msvc"
$Tag = Resolve-ReleaseTag
if (-not $Tag) {
    if ($env:DRY_RUN -eq "1") {
        Write-Host "[dry-run] No GitHub release found for $Repo; would install from source"
        Write-Host "[dry-run] Source URL: https://github.com/$Repo.git"
        Write-Host "[dry-run] Command: cargo install --git https://github.com/$Repo.git --bin zaion --locked --force"
        exit 0
    }
    Install-FromSource
    Write-Host ""
    Write-Host "Next steps:"
    Write-Host "  zaion onboard"
    Write-Host "  zaion doctor"
    Write-Host "  zaion chat `"Hello`""
    Write-Host "  zaion tui"
    Write-Host ""
    Write-Host "No interactive onboarding was started automatically."
    exit 0
}
$ArchiveName = "zaion-$Tag-$Target.zip"
$DownloadUrl = "https://github.com/$Repo/releases/download/$Tag/$ArchiveName"
$ChecksumUrl = "$DownloadUrl.sha256"
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) "zaion-install-$([Guid]::NewGuid().ToString('N'))"

if ($env:DRY_RUN -eq "1") {
    Write-Host "[dry-run] Would install Zaion for windows/x86_64"
    Write-Host "[dry-run] Release tag: $Tag"
    Write-Host "[dry-run] Download URL: $DownloadUrl"
    Write-Host "[dry-run] Checksum URL: $ChecksumUrl"
    Write-Host "[dry-run] Install path: $(Join-Path $InstallDir 'zaion.exe')"
    exit 0
}

Write-Host "Installing Zaion..."
Write-Host "  OS:      windows"
Write-Host "  Arch:    x86_64"
Write-Host "  Target:  $Target"
Write-Host "  Release: $Tag"

New-Item -ItemType Directory -Force -Path $TempDir | Out-Null
try {
    $ArchivePath = Join-Path $TempDir $ArchiveName
    $ChecksumPath = "$ArchivePath.sha256"
    Download-File $DownloadUrl $ArchivePath "release archive"
    Download-File $ChecksumUrl $ChecksumPath "checksum file"

    $ChecksumLines = @(
        Get-Content -LiteralPath $ChecksumPath |
            ForEach-Object { $_.Trim() } |
            Where-Object { $_ }
    )
    if ($ChecksumLines.Count -ne 1) {
        Fail "Checksum file must contain exactly one non-empty record: $ChecksumPath"
    }
    if ($ChecksumLines[0] -notmatch '^([A-Fa-f0-9]{64})\s+(\S+)$') {
        Fail "Checksum file must use '<64-char-sha256>  <archive-name>': $ChecksumPath"
    }
    $Expected = $Matches[1]
    $ChecksumArchiveName = $Matches[2]
    if ($Expected -match '^0{64}$') {
        Fail "Checksum file contains a placeholder SHA-256 digest: $ChecksumPath"
    }
    if ($ChecksumArchiveName -ne $ArchiveName) {
        Fail "Checksum archive name mismatch: expected $ArchiveName, found $ChecksumArchiveName"
    }
    $Actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $ArchivePath).Hash.ToLowerInvariant()
    if ($Expected.ToLowerInvariant() -ne $Actual) {
        Fail "Checksum verification failed for $ArchiveName. Expected $Expected, actual $Actual."
    }
    Write-Host "Checksum verified: $ArchiveName"

    $ExtractDir = Join-Path $TempDir "extract"
    Expand-Archive -LiteralPath $ArchivePath -DestinationPath $ExtractDir -Force
    $Binary = Join-Path $ExtractDir "zaion.exe"
    if (-not (Test-Path -LiteralPath $Binary)) {
        Fail "Release archive did not contain zaion.exe."
    }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Copy-Item -LiteralPath $Binary -Destination (Join-Path $InstallDir "zaion.exe") -Force
    Add-ToUserPath $InstallDir

    Write-Host "Installed zaion to $(Join-Path $InstallDir 'zaion.exe')"
    Write-Host "Verification: $(& (Join-Path $InstallDir 'zaion.exe') --version)"
    Write-Host ""
    Write-Host "Next steps:"
    Write-Host "  zaion onboard"
    Write-Host "  zaion doctor"
    Write-Host "  zaion chat `"Hello`""
    Write-Host "  zaion tui"
    Write-Host ""
    Write-Host "Close and reopen PowerShell, Windows Terminal, VS Code, or your IDE terminal if an existing shell cannot find zaion."
    Write-Host "No interactive onboarding was started automatically."
} finally {
    Remove-Item -LiteralPath $TempDir -Recurse -Force -ErrorAction SilentlyContinue
}
