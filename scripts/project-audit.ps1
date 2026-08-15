[CmdletBinding()]
param(
    [int]$LargeRustFileLines = 1000,
    [switch]$IncludeDiskUsage,
    [switch]$Strict
)

Set-StrictMode -Version 2.0
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$warnings = New-Object System.Collections.Generic.List[string]

function Write-Section {
    param([string]$Title)
    Write-Output ""
    Write-Output "== $Title =="
}

function Add-AuditWarning {
    param([string]$Message)
    $script:warnings.Add($Message)
    Write-Output "WARN: $Message"
}

function Get-TreeSizeMiB {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) {
        return 0
    }
    $sum = (Get-ChildItem -LiteralPath $Path -Recurse -File -Force -ErrorAction SilentlyContinue |
        Measure-Object -Property Length -Sum).Sum
    if ($null -eq $sum) {
        return 0
    }
    return [math]::Round(($sum / 1MB), 2)
}

Push-Location $root
try {
    Write-Output "Zaion project audit"
    Write-Output "root: $root"
    Write-Output "time: $([DateTimeOffset]::Now.ToString('o'))"

    Write-Section "Git"
    $branch = (& git branch --show-current | Out-String).Trim()
    $head = (& git rev-parse HEAD | Out-String).Trim()
    $status = @(& git status --short)
    Write-Output "branch: $branch"
    Write-Output "head:   $head"
    Write-Output "dirty entries: $($status.Count)"
    if ($status.Count -gt 0) {
        $status | Select-Object -First 80 | ForEach-Object { Write-Output "  $_" }
        if ($status.Count -gt 80) {
            Write-Output "  ... $($status.Count - 80) more"
        }
    }

    $remotes = @(& git remote)
    if ($remotes.Count -eq 0) {
        Add-AuditWarning "No Git remote is configured for this workspace."
    }

    Write-Section "Cargo workspace"
    $metadataText = (& cargo metadata --no-deps --format-version 1 | Out-String)
    if ($LASTEXITCODE -ne 0) {
        throw "cargo metadata failed"
    }
    $metadata = $metadataText | ConvertFrom-Json
    $packages = @($metadata.packages)
    Write-Output "crates: $($packages.Count)"
    $missingRustVersion = @($packages | Where-Object { [string]::IsNullOrWhiteSpace($_.rust_version) })
    $declaredRustVersions = @($packages | ForEach-Object { $_.rust_version } |
        Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
        Sort-Object -Unique)
    Write-Output "declared rust-version: $($declaredRustVersions -join ', ')"
    if ($missingRustVersion.Count -gt 0) {
        Add-AuditWarning "$($missingRustVersion.Count) workspace crates do not declare rust-version."
    }
    if ($declaredRustVersions.Count -gt 1) {
        Add-AuditWarning "Workspace crates declare multiple rust-version values: $($declaredRustVersions -join ', ')."
    }

    $crateRows = @()
    foreach ($package in $packages) {
        $crateDir = Split-Path -Parent $package.manifest_path
        $sourceFiles = @(Get-ChildItem -LiteralPath (Join-Path $crateDir "src") -Recurse -Filter "*.rs" -File -ErrorAction SilentlyContinue)
        $testFiles = @(Get-ChildItem -LiteralPath (Join-Path $crateDir "tests") -Recurse -Filter "*.rs" -File -ErrorAction SilentlyContinue)
        $sourceLines = 0
        foreach ($file in $sourceFiles) {
            $sourceLines += @(Get-Content -LiteralPath $file.FullName).Count
        }
        $testLines = 0
        foreach ($file in $testFiles) {
            $testLines += @(Get-Content -LiteralPath $file.FullName).Count
        }
        $internalDependencies = @($package.dependencies | Where-Object {
                $_.PSObject.Properties.Name -contains "path" -and $null -ne $_.path
            }).Count
        $crateRows += [pscustomobject]@{
            Crate = $package.name
            SourceFiles = $sourceFiles.Count
            SourceLines = $sourceLines
            TestFiles = $testFiles.Count
            TestLines = $testLines
            InternalDependencies = $internalDependencies
        }
    }
    $sourceTotal = ($crateRows | Measure-Object -Property SourceLines -Sum).Sum
    $testTotal = ($crateRows | Measure-Object -Property TestLines -Sum).Sum
    Write-Output "source lines: $sourceTotal"
    Write-Output "test-directory lines: $testTotal"
    Write-Output "largest crates:"
    $crateRows | Sort-Object SourceLines -Descending | Select-Object -First 12 |
        Format-Table -AutoSize | Out-String | Write-Output

    $reverseDependencies = @{}
    foreach ($package in $packages) {
        $reverseDependencies[$package.name] = New-Object System.Collections.Generic.List[string]
    }
    foreach ($package in $packages) {
        foreach ($dependency in @($package.dependencies | Where-Object {
                    $_.PSObject.Properties.Name -contains "path" -and $null -ne $_.path
                })) {
            $reverseDependencies[$dependency.name].Add($package.name)
        }
    }
    $leafPackages = @($packages | Where-Object { $reverseDependencies[$_.name].Count -eq 0 } | ForEach-Object { $_.name } | Sort-Object)
    Write-Output "workspace packages with no workspace consumers: $($leafPackages -join ', ')"

    Write-Section "Large Rust files"
    $largeFiles = @()
    foreach ($file in @(Get-ChildItem -LiteralPath "crates" -Recurse -Filter "*.rs" -File)) {
        $lineCount = @(Get-Content -LiteralPath $file.FullName).Count
        if ($lineCount -ge $LargeRustFileLines) {
            $largeFiles += [pscustomobject]@{
                Lines = $lineCount
                Path = $file.FullName.Substring($root.Length + 1)
            }
        }
    }
    Write-Output "threshold: $LargeRustFileLines lines"
    Write-Output "matching files: $($largeFiles.Count)"
    $largeFiles | Sort-Object Lines -Descending | Select-Object -First 40 |
        Format-Table -AutoSize | Out-String | Write-Output

    Write-Section "Repository shape checks"
    $websiteExists = Test-Path -LiteralPath "zaion-website"
    $ciText = Get-Content -LiteralPath ".github/workflows/ci.yml" -Raw
    $readmeText = Get-Content -LiteralPath "README.md" -Raw
    Write-Output "zaion-website exists: $websiteExists"
    if (-not $websiteExists -and ($ciText.Contains("zaion-website") -or $readmeText.Contains("zaion-website"))) {
        Add-AuditWarning "zaion-website is absent, but README and/or CI still reference it."
    }

    $hookPaths = @(
        ".claude/hooks/inject-context.sh",
        ".claude/hooks/pre-tool-guard.sh",
        ".claude/hooks/stop-verify.sh"
    )
    $settingsText = ""
    if (Test-Path -LiteralPath ".claude/settings.json") {
        $settingsText = Get-Content -LiteralPath ".claude/settings.json" -Raw
    }
    foreach ($hookPath in $hookPaths) {
        if ($settingsText.Contains($hookPath.Replace(".claude/", ".claude/")) -and -not (Test-Path -LiteralPath $hookPath)) {
            Add-AuditWarning "Claude settings reference missing hook: $hookPath"
        }
    }

    $activeLedgers = @(
        "MASTER_PLAN.md",
        "plans/openclaw_latest_gap_report.md",
        "plans/hermes_surpass_master_plan.md"
    )
    foreach ($ledger in $activeLedgers) {
        $ledgerText = Get-Content -LiteralPath $ledger -Encoding utf8 -Raw
        $replacementCount = ([regex]::Matches($ledgerText, [string][char]0xFFFD)).Count
        $questionPairCount = ([regex]::Matches($ledgerText, "\?\?")).Count
        Write-Output "$ledger : replacement_chars=$replacementCount suspicious_question_pairs=$questionPairCount"
        if ($replacementCount -gt 0) {
            Add-AuditWarning "$ledger contains Unicode replacement characters."
        }
    }

    $strictUtf8 = New-Object System.Text.UTF8Encoding -ArgumentList $false, $true
    $markdownFiles = @((Get-Item -LiteralPath "README.md"), (Get-Item -LiteralPath "MASTER_PLAN.md")) +
        @(Get-ChildItem -LiteralPath "docs", "plans" -Recurse -Filter "*.md" -File)
    foreach ($markdownFile in $markdownFiles) {
        try {
            $bytes = [System.IO.File]::ReadAllBytes($markdownFile.FullName)
            $null = $strictUtf8.GetString($bytes)
        }
        catch {
            Add-AuditWarning "$($markdownFile.FullName.Substring($root.Length + 1)) is not valid UTF-8."
        }
    }

    $trackedTargetArtifacts = @(& git ls-files "*/target/*")
    if ($trackedTargetArtifacts.Count -gt 0) {
        $presentTrackedTargetArtifacts = @($trackedTargetArtifacts | Where-Object { Test-Path -LiteralPath $_ })
        $pendingTargetDeletions = @($trackedTargetArtifacts | Where-Object { -not (Test-Path -LiteralPath $_) })
        if ($presentTrackedTargetArtifacts.Count -gt 0) {
            Add-AuditWarning "Tracked target/ artifacts still present: $($presentTrackedTargetArtifacts.Count)."
            $presentTrackedTargetArtifacts | Select-Object -First 30 | ForEach-Object { Write-Output "  tracked target: $_" }
        }
        if ($pendingTargetDeletions.Count -gt 0) {
            Write-Output "tracked target/ artifacts pending deletion: $($pendingTargetDeletions.Count)"
        }
    }

    $trackedLocalSettings = @(& git ls-files ".claude/settings.local.json")
    if ($trackedLocalSettings.Count -gt 0) {
        Add-AuditWarning ".claude/settings.local.json is tracked even though it is machine-specific."
    }

    Write-Section "Registered worktrees"
    & git worktree list

    $hermesMirror = "D:/zaion-reference/hermes-agent-latest"
    if (Test-Path -LiteralPath $hermesMirror) {
        Write-Section "Hermes local mirror"
        $hermesHead = (& git -C $hermesMirror rev-parse HEAD | Out-String).Trim()
        $hermesLog = (& git -C $hermesMirror log -1 --date=iso --pretty=format:"%H%n%ad%n%s" | Out-String).Trim()
        Write-Output "head: $hermesHead"
        Write-Output $hermesLog
    }

    if ($IncludeDiskUsage) {
        Write-Section "Disk usage"
        foreach ($path in @("target", ".claude/worktrees", "claude-code-source", "zaion-data", "plans", "docs", "crates")) {
            if (Test-Path -LiteralPath $path) {
                Write-Output ("{0,-24} {1,10} MiB" -f $path, (Get-TreeSizeMiB $path))
            }
        }
        if (Test-Path -LiteralPath "claude-code-source.zip") {
            $archiveSize = [math]::Round(((Get-Item -LiteralPath "claude-code-source.zip").Length / 1MB), 2)
            Write-Output ("{0,-24} {1,10} MiB" -f "claude-code-source.zip", $archiveSize)
        }
    }

    Write-Section "Summary"
    Write-Output "warnings: $($warnings.Count)"
    foreach ($warning in $warnings) {
        Write-Output "  - $warning"
    }
}
finally {
    Pop-Location
}

if ($Strict -and $warnings.Count -gt 0) {
    exit 1
}
