[CmdletBinding()]
param(
    [string]$BenchmarkPath = "eval/benchmarks/zaion_300_v1.json",
    [string]$SchemaPath = "eval/benchmark_manifest.schema.json",
    [string]$ScorecardPath = "docs/PRODUCT_SCORECARD.md",
    [string]$ThreatModelPath = "docs/THREAT_MODEL.md"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$failures = New-Object System.Collections.Generic.List[string]
$allowedStatuses = @("planned", "ready", "running", "verified", "blocked", "retired")
$benchmarkId = "<unavailable>"
$targetSlots = 0
$claimedVerifiedSlots = 0
$manifestStatus = "<unavailable>"
$hermesCommit = "<unavailable>"
$categoryWeightTotal = [decimal]0
$riskWeightTotal = 0
$taskSlotTotal = 0
$verifiedSlotTotal = 0

function Add-Failure {
    param([string]$Message)
    $script:failures.Add($Message)
}

function Resolve-RepositoryPath {
    param([string]$Path)
    if ([System.IO.Path]::IsPathRooted($Path)) {
        return $Path
    }
    return Join-Path $script:root $Path
}

function Test-ObjectProperty {
    param(
        [object]$Object,
        [string]$Name
    )
    return $null -ne $Object -and $null -ne $Object.PSObject.Properties[$Name]
}

function Get-RequiredValue {
    param(
        [object]$Object,
        [string]$Name,
        [string]$Context
    )
    if (-not (Test-ObjectProperty -Object $Object -Name $Name)) {
        Add-Failure "$Context is missing required field '$Name'."
        return $null
    }
    $value = $Object.PSObject.Properties[$Name].Value
    if ($null -eq $value) {
        Add-Failure "$Context field '$Name' must not be null."
        return $null
    }
    if ($value -is [string] -and [string]::IsNullOrWhiteSpace($value)) {
        Add-Failure "$Context field '$Name' must not be blank."
        return $null
    }
    return $value
}

function Read-JsonFile {
    param(
        [string]$Path,
        [string]$Label
    )
    try {
        return Get-Content -LiteralPath $Path -Raw -Encoding utf8 | ConvertFrom-Json
    }
    catch {
        Add-Failure "$Label is not valid JSON: $($_.Exception.Message)"
        return $null
    }
}

function Assert-AcceptanceTier {
    param(
        [object]$Acceptance,
        [string]$Tier,
        [string]$Context
    )
    if (-not (Test-ObjectProperty -Object $Acceptance -Name $Tier)) {
        Add-Failure "$Context acceptance is missing '$Tier'."
        return
    }
    $items = @($Acceptance.PSObject.Properties[$Tier].Value)
    if ($items.Count -eq 0) {
        Add-Failure "$Context acceptance '$Tier' must contain at least one criterion."
        return
    }
    foreach ($item in $items) {
        if ([string]::IsNullOrWhiteSpace([string]$item)) {
            Add-Failure "$Context acceptance '$Tier' contains a blank criterion."
        }
    }
}

$requiredFiles = @(
    $BenchmarkPath,
    $SchemaPath,
    $ScorecardPath,
    $ThreatModelPath,
    "eval/README.md"
)

Push-Location $root
try {
    foreach ($path in $requiredFiles) {
        if (-not (Test-Path -LiteralPath (Resolve-RepositoryPath $path) -PathType Leaf)) {
            Add-Failure "Required product-gate asset is missing: $path"
        }
    }

    if ($failures.Count -gt 0) {
        throw "Required assets are missing."
    }

    $manifest = Read-JsonFile -Path (Resolve-RepositoryPath $BenchmarkPath) -Label "Benchmark manifest"
    $schema = Read-JsonFile -Path (Resolve-RepositoryPath $SchemaPath) -Label "Benchmark schema"
    $scorecardText = Get-Content -LiteralPath (Resolve-RepositoryPath $ScorecardPath) -Raw -Encoding utf8
    $threatModelText = Get-Content -LiteralPath (Resolve-RepositoryPath $ThreatModelPath) -Raw -Encoding utf8

    if ($null -eq $manifest -or $null -eq $schema) {
        throw "JSON parsing failed."
    }

    if (-not (Test-ObjectProperty -Object $schema -Name '$schema')) {
        Add-Failure "Benchmark schema does not declare a JSON Schema dialect."
    }

    $benchmarkId = [string](Get-RequiredValue $manifest "benchmark_id" "manifest")
    $targetSlots = [int](Get-RequiredValue $manifest "target_task_slots" "manifest")
    $claimedVerifiedSlots = [int](Get-RequiredValue $manifest "claimed_verified_slots" "manifest")
    $manifestStatus = [string](Get-RequiredValue $manifest "status" "manifest")

    $scorePolicy = Get-RequiredValue $manifest "score_policy" "manifest"
    $riskWeights = if ($null -ne $scorePolicy) {
        Get-RequiredValue $scorePolicy "risk_adjusted_weights" "manifest.score_policy"
    }
    else {
        $null
    }
    $expectedRiskWeights = [ordered]@{
        task_success = 40
        no_human_rework = 20
        recovery = 15
        trust_verification = 15
        cost_latency = 10
    }
    $riskWeightTotal = 0
    if ($null -ne $riskWeights) {
        foreach ($entry in $expectedRiskWeights.GetEnumerator()) {
            $value = Get-RequiredValue $riskWeights $entry.Key "manifest.score_policy.risk_adjusted_weights"
            if ($null -eq $value) {
                continue
            }
            try {
                $numericValue = [int]$value
                $riskWeightTotal += $numericValue
                if ($numericValue -ne $entry.Value) {
                    Add-Failure "Risk-adjusted weight '$($entry.Key)' must be $($entry.Value); observed $numericValue."
                }
            }
            catch {
                Add-Failure "Risk-adjusted weight '$($entry.Key)' must be an integer."
            }
        }
        $unexpectedWeights = @($riskWeights.PSObject.Properties.Name | Where-Object {
            -not $expectedRiskWeights.Contains($_)
        })
        if ($unexpectedWeights.Count -gt 0) {
            Add-Failure "Unexpected risk-adjusted weight(s): $($unexpectedWeights -join ', ')."
        }
    }
    if ($riskWeightTotal -ne 100) {
        Add-Failure "Risk-adjusted task weights must total 100; observed $riskWeightTotal."
    }

    $baselines = Get-RequiredValue $manifest "baselines" "manifest"
    $hermes = if ($null -ne $baselines) {
        Get-RequiredValue $baselines "hermes" "manifest.baselines"
    }
    else {
        $null
    }
    $hermesCommit = if ($null -ne $hermes) {
        [string](Get-RequiredValue $hermes "commit" "manifest.baselines.hermes")
    }
    else {
        ""
    }
    if ($hermesCommit -notmatch '^[0-9a-f]{40}$') {
        Add-Failure "Hermes baseline commit must be exactly 40 lowercase hexadecimal characters."
    }
    if ($null -ne $hermes) {
        $null = Get-RequiredValue $hermes "ref" "manifest.baselines.hermes"
        $null = Get-RequiredValue $hermes "repository" "manifest.baselines.hermes"
        $null = Get-RequiredValue $hermes "mirror" "manifest.baselines.hermes"
        $hermesSources = @((Get-RequiredValue $hermes "source" "manifest.baselines.hermes"))
        if ($hermesSources.Count -eq 0) {
            Add-Failure "Hermes baseline must name at least one source anchor."
        }
    }

    $categories = @((Get-RequiredValue $manifest "categories" "manifest"))
    $categoryById = @{}
    $categoryWeightTotal = [decimal]0
    $categorySlotTotal = 0
    foreach ($category in $categories) {
        $categoryId = [string](Get-RequiredValue $category "id" "category")
        if ([string]::IsNullOrWhiteSpace($categoryId)) {
            continue
        }
        if ($categoryById.ContainsKey($categoryId)) {
            Add-Failure "Duplicate category id: $categoryId"
            continue
        }
        $categoryById[$categoryId] = $category
        try {
            $weight = [decimal](Get-RequiredValue $category "weight" "category '$categoryId'")
            $categoryWeightTotal += $weight
        }
        catch {
            Add-Failure "Category '$categoryId' has a non-numeric weight."
        }
        try {
            $slots = [int](Get-RequiredValue $category "task_slots" "category '$categoryId'")
            if ($slots -le 0) {
                Add-Failure "Category '$categoryId' task_slots must be positive."
            }
            $categorySlotTotal += $slots
        }
        catch {
            Add-Failure "Category '$categoryId' has non-integer task_slots."
        }
        foreach ($gate in @("parity_gate", "surpass_gate", "ten_out_of_ten")) {
            $null = Get-RequiredValue $category $gate "category '$categoryId'"
        }
    }

    if ($categoryWeightTotal -ne [decimal]100) {
        Add-Failure "Category weights must total 100; observed $categoryWeightTotal."
    }
    if ($categorySlotTotal -ne $targetSlots) {
        Add-Failure "Category task_slots total $categorySlotTotal but target_task_slots is $targetSlots."
    }

    $tasks = @((Get-RequiredValue $manifest "tasks" "manifest"))
    $taskIds = @{}
    $taskSlotTotal = 0
    $verifiedSlotTotal = 0
    $slotsByCategory = @{}
    foreach ($categoryId in $categoryById.Keys) {
        $slotsByCategory[$categoryId] = 0
    }

    foreach ($task in $tasks) {
        $taskId = [string](Get-RequiredValue $task "id" "task")
        $context = if ([string]::IsNullOrWhiteSpace($taskId)) { "task" } else { "task '$taskId'" }
        if (-not [string]::IsNullOrWhiteSpace($taskId)) {
            if ($taskIds.ContainsKey($taskId)) {
                Add-Failure "Duplicate task id: $taskId"
            }
            else {
                $taskIds[$taskId] = $true
            }
        }

        $categoryId = [string](Get-RequiredValue $task "category" $context)
        $status = [string](Get-RequiredValue $task "status" $context)
        $source = Get-RequiredValue $task "source" $context
        $acceptance = Get-RequiredValue $task "acceptance" $context
        $null = Get-RequiredValue $task "title" $context

        if ($allowedStatuses -notcontains $status) {
            Add-Failure "$context has unsupported status '$status'."
        }
        if (-not $categoryById.ContainsKey($categoryId)) {
            Add-Failure "$context references unknown category '$categoryId'."
        }

        try {
            $slots = [int](Get-RequiredValue $task "slots" $context)
            if ($slots -le 0) {
                Add-Failure "$context slots must be positive."
            }
            $taskSlotTotal += $slots
            if ($slotsByCategory.ContainsKey($categoryId)) {
                $slotsByCategory[$categoryId] += $slots
            }
        }
        catch {
            $slots = 0
            Add-Failure "$context has non-integer slots."
        }

        if ($null -ne $source) {
            foreach ($field in @("kind", "repository", "path", "ref", "commit")) {
                $null = Get-RequiredValue $source $field "$context source"
            }
            $sourceKind = [string]$source.kind
            if ($sourceKind -eq "hermes_source") {
                $sourceCommit = [string]$source.commit
                if ($sourceCommit -ne $hermesCommit) {
                    Add-Failure "$context Hermes source commit does not match the pinned baseline."
                }
            }
        }

        if ($null -ne $acceptance) {
            Assert-AcceptanceTier $acceptance "parity" $context
            Assert-AcceptanceTier $acceptance "surpass" $context
            Assert-AcceptanceTier $acceptance "ten_out_of_ten" $context
        }

        if (-not (Test-ObjectProperty $task "score")) {
            Add-Failure "$context is missing required field 'score'."
            $score = $null
        }
        else {
            $score = $task.score
        }
        if (-not (Test-ObjectProperty $task "evidence")) {
            Add-Failure "$context is missing required field 'evidence'."
            $evidence = @()
        }
        else {
            $evidence = @($task.evidence)
        }
        if (-not (Test-ObjectProperty $task "result")) {
            Add-Failure "$context is missing required field 'result'."
            $result = $null
        }
        else {
            $result = $task.result
        }

        if ($null -ne $score) {
            try {
                $numericScore = [decimal]$score
                if ($numericScore -lt 0 -or $numericScore -gt 10) {
                    Add-Failure "$context score must be between 0 and 10."
                }
            }
            catch {
                Add-Failure "$context score is not numeric."
                $numericScore = $null
            }
        }
        else {
            $numericScore = $null
        }

        if ($status -ne "verified" -and $null -ne $score) {
            Add-Failure "$context is not verified and therefore must keep score null."
        }

        if ($status -eq "verified") {
            if ($evidence.Count -eq 0) {
                Add-Failure "$context is verified but has no evidence."
            }
            if ($null -eq $result) {
                Add-Failure "$context is verified but has no result object."
            }
            else {
                $resultSlots = [int](Get-RequiredValue $result "verified_slots" "$context result")
                $resultScore = Get-RequiredValue $result "score" "$context result"
                $evidenceGrade = [string](Get-RequiredValue $result "evidence_grade" "$context result")
                $null = Get-RequiredValue $result "summary" "$context result"
                if ($resultSlots -ne $slots) {
                    Add-Failure "$context result verifies $resultSlots slots but the task represents $slots."
                }
                if ($null -eq $score -or [decimal]$resultScore -ne [decimal]$score) {
                    Add-Failure "$context result score must match the task score."
                }
                $verifiedSlotTotal += $resultSlots
            }

            foreach ($item in $evidence) {
                foreach ($field in @("id", "kind", "path", "result", "observed_at")) {
                    $null = Get-RequiredValue $item $field "$context evidence"
                }
                if ([string]$item.result -ne "pass") {
                    Add-Failure "$context verified evidence must have result 'pass'."
                }
                $evidencePath = Resolve-RepositoryPath ([string]$item.path)
                if (-not (Test-Path -LiteralPath $evidencePath -PathType Leaf)) {
                    Add-Failure "$context evidence artifact does not exist: $($item.path)"
                }
            }
        }

        if ($null -ne $numericScore -and $numericScore -eq 10) {
            if ($status -ne "verified" -or $evidence.Count -eq 0 -or $null -eq $result) {
                Add-Failure "$context claims 10/10 without verified evidence."
            }
            elseif ([string]$result.evidence_grade -ne "release_verified") {
                Add-Failure "$context claims 10/10 without release_verified evidence."
            }
        }
    }

    if ($taskSlotTotal -ne $targetSlots) {
        Add-Failure "Task slots total $taskSlotTotal but target_task_slots is $targetSlots."
    }
    foreach ($categoryId in $categoryById.Keys) {
        $expected = [int]$categoryById[$categoryId].task_slots
        $actual = [int]$slotsByCategory[$categoryId]
        if ($actual -ne $expected) {
            Add-Failure "Category '$categoryId' reserves $expected slots but its tasks total $actual."
        }
    }
    if ($verifiedSlotTotal -ne $claimedVerifiedSlots) {
        Add-Failure "Evidence-backed verified slots total $verifiedSlotTotal but claimed_verified_slots is $claimedVerifiedSlots."
    }
    if ($manifestStatus -eq "scaffold" -and $claimedVerifiedSlots -ne 0) {
        Add-Failure "A scaffold manifest cannot claim verified slots."
    }

    $scorecardPattern = '(?m)^\|\s*\x60(?<id>[a-z0-9_-]+)\x60\s*\|\s*(?<weight>\d+(?:\.\d+)?)\s*\|'
    $scorecardMatches = [regex]::Matches($scorecardText, $scorecardPattern)
    $scorecardWeights = @{}
    $scorecardWeightTotal = [decimal]0
    foreach ($match in $scorecardMatches) {
        $id = $match.Groups["id"].Value
        $weight = [decimal]$match.Groups["weight"].Value
        if ($scorecardWeights.ContainsKey($id)) {
            Add-Failure "Scorecard contains duplicate category row '$id'."
        }
        else {
            $scorecardWeights[$id] = $weight
            $scorecardWeightTotal += $weight
        }
    }
    if ($scorecardWeightTotal -ne [decimal]100) {
        Add-Failure "Scorecard weights must total 100; observed $scorecardWeightTotal."
    }
    foreach ($categoryId in $categoryById.Keys) {
        if (-not $scorecardWeights.ContainsKey($categoryId)) {
            Add-Failure "Scorecard is missing category '$categoryId'."
        }
        elseif ([decimal]$scorecardWeights[$categoryId] -ne [decimal]$categoryById[$categoryId].weight) {
            Add-Failure "Scorecard weight for '$categoryId' does not match the manifest."
        }
    }
    if (-not $scorecardText.Contains($hermesCommit)) {
        Add-Failure "Scorecard does not name the pinned Hermes commit."
    }
    if (-not $threatModelText.Contains("## Trust Boundaries") -or
        -not $threatModelText.Contains("## Threat Register") -or
        -not $threatModelText.Contains("## Security Invariants")) {
        Add-Failure "Threat model is missing a required governance section."
    }

    if ($null -ne $hermes -and (Test-Path -LiteralPath ([string]$hermes.mirror) -PathType Container)) {
        $mirrorHead = (& git -C ([string]$hermes.mirror) rev-parse HEAD | Out-String).Trim()
        if ($LASTEXITCODE -ne 0) {
            Add-Failure "Unable to read the local Hermes mirror HEAD."
        }
        elseif ($mirrorHead -ne $hermesCommit) {
            Add-Failure "Local Hermes mirror HEAD '$mirrorHead' does not match baseline '$hermesCommit'."
        }
    }
}
catch {
    if ($failures.Count -eq 0) {
        Add-Failure $_.Exception.Message
    }
}
finally {
    Pop-Location
}

Write-Output "Zaion product gate validation"
Write-Output "benchmark:       $benchmarkId"
Write-Output "category weight: $categoryWeightTotal"
Write-Output "mission weights: $riskWeightTotal"
Write-Output "task slots:      $taskSlotTotal / $targetSlots"
Write-Output "verified slots:  $verifiedSlotTotal / $targetSlots"
Write-Output "Hermes commit:   $hermesCommit"

if ($failures.Count -gt 0) {
    Write-Output "result:          FAIL ($($failures.Count) issue(s))"
    foreach ($failure in $failures) {
        Write-Output "  - $failure"
    }
    exit 1
}

Write-Output "result:          PASS"
