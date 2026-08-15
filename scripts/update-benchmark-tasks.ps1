$ErrorActionPreference = 'Stop'
$path = 'D:\zaion-rust\eval\benchmarks\zaion_300_v1.json'
$j = Get-Content $path -Raw | ConvertFrom-Json

# 1. align existing task slots to new category allocation
$slotMap = @{ onboarding=15; tui=18; session=18; tools=24; skills=12; memory=18; context=18; gateway=18; channels=18; mcp=15; acp=15; environments=15; batch_eval=12; release=15; community=9 }
foreach ($t in $j.tasks) {
  if ($slotMap.ContainsKey($t.category)) { $t.slots = $slotMap[$t.category] }
}

# 2. scaffold tasks for new categories
$heroScaffold = [pscustomobject]@{
  id = 'ZAION-300-HERO-MISSION'; category = 'hero_mission'; slots = 30
  title = 'Materialize dev/SRE hero-mission loop: alert to investigation, fix, approval, execution, verification, rollback, signed evidence'
  status = 'planned'
  source = @{ kind = 'zaion_spec'; path = 'plans/zaion-10-10-leap-plan.md'; ref = 'main' }
  acceptance = @{
    parity = @('Issue/alert to investigation and fix completes end to end.', 'Approval, execution, verification, and rollback are actionable.')
    surpass = @('Signed evidence pack is independently verifiable.')
    ten_out_of_ten = @('First real mission under 15 minutes; 100 percent verifiable; zero silent failures.')
  }
  score = $null; evidence = @(); result = $null
  task_type = 'happy_path'
}
$relScaffold = [pscustomobject]@{
  id = 'ZAION-300-RELIABILITY'; category = 'reliability_security'; slots = 30
  title = 'Materialize reliability and security chaos scenarios from the plan mandatory-test list'
  status = 'planned'
  source = @{ kind = 'zaion_spec'; path = 'plans/zaion-10-10-leap-plan.md'; ref = 'main' }
  acceptance = @{
    parity = @('Crash at each event-commit point, out-of-order events, disk full, signature tampering, IDOR, sandbox escape are handled.')
    surpass = @('Failures are structured, user-visible, auditable; no double side effects.')
    ten_out_of_ten = @('Required chaos scenarios pass with zero data loss or cross-tenant leakage.')
  }
  score = $null; evidence = @(); result = $null
  task_type = 'recovery'
}

# 3. concrete hero seeds
function New-HeroTask($id, $cat, $title, $type, $parity, $surpass, $ten) {
  [pscustomobject]@{
    id = $id; category = $cat; slots = 1; title = $title; status = 'planned'
    source = @{ kind = 'zaion_spec'; path = 'plans/zaion-10-10-leap-plan.md'; ref = 'main' }
    acceptance = @{ parity = @($parity); surpass = @($surpass); ten_out_of_ten = @($ten) }
    score = $null; evidence = @(); result = $null; task_type = $type
  }
}
$seeds = @(
  (New-HeroTask 'ZAION-300-HERO-001' 'hero_mission' 'Given an alert or failing test, locate the root cause and propose a minimal fix' 'happy_path' 'Agent reads the alert and repository, identifies root cause with evidence.' 'Investigation steps are auditable and the fix is minimal.' 'Median root-cause time under 10 minutes in the sandbox repo.'),
  (New-HeroTask 'ZAION-300-HERO-002' 'hero_mission' 'A production config change requires approval before execution' 'approval' 'Agent prepares the change and requests approval; execution happens only after approval.' 'Denied approval aborts with no side effects.' 'Approval flow completes with a signed decision record.'),
  (New-HeroTask 'ZAION-300-HERO-003' 'hero_mission' 'Apply a fix, run tests, and produce a signed evidence pack' 'evidence' 'Agent modifies code, runs verification, and emits a signed evidence pack with proof closure.' 'Evidence pack is independently verifiable.' '100 percent of successful fixes are verifiable.'),
  (New-HeroTask 'ZAION-300-HERO-004' 'hero_mission' 'A deployed change causes failure; roll back to the last known-good state' 'recovery' 'Agent detects the regression and rolls back the change.' 'Rollback restores known-good state with no data loss.' 'Rollback completes and evidence records the recovery.'),
  (New-HeroTask 'ZAION-300-HERO-005' 'hero_mission' 'Interrupt a long-running mission, then resume it across a new session' 'recovery' 'Agent preserves context and evidence across interruption; resume continues correctly.' 'No duplicated side effects after resume.' 'Resume keeps context and completes the mission.')
)
$j.tasks = @($j.tasks) + @($heroScaffold, $relScaffold) + $seeds

$json = $j | ConvertTo-Json -Depth 20
[System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
Write-Output 'manifest tasks updated'
