$ErrorActionPreference = 'Stop'
$path = 'D:\zaion-rust\eval\benchmarks\zaion_300_v1.json'
$j = Get-Content $path -Raw | ConvertFrom-Json

# 1. Update baselines: refresh hermes, add openclaw
$j.baselines.hermes | Add-Member -NotePropertyName upstream_commit -NotePropertyValue '1f8fdc7bd8240' -Force
$j.baselines.hermes | Add-Member -NotePropertyName upstream_date -NotePropertyValue '2026-08-14 11:23:40 UTC' -Force
$j.baselines.hermes | Add-Member -NotePropertyName mirror_status -NotePropertyValue 'stale_local_mirror_9c080707' -Force
$openclaw = [pscustomobject]@{
  name = 'OpenClaw'
  repository = 'https://github.com/openclaw/openclaw.git'
  ref = 'main'
  commit = 'c3ae887f465a'
  commit_date = '2026-08-14 11:51:26 UTC'
  status = 'source_calibrated'
}
$j.baselines | Add-Member -NotePropertyName openclaw -NotePropertyValue $openclaw -Force

# 2. Update category task_slots per taxonomy proposal
$newSlots = @{
  onboarding = 15; tui = 18; session = 18; tools = 24; skills = 12; memory = 18
  context = 18; gateway = 18; channels = 18; mcp = 15; acp = 15; environments = 15
  batch_eval = 12; release = 15; community = 9
}
foreach ($c in $j.categories) {
  if ($newSlots.ContainsKey($c.id)) { $c.task_slots = $newSlots[$c.id] }
}

# 3. Add hero_mission + reliability_security categories
$hero = [pscustomobject]@{
  id = 'hero_mission'; label = 'Hero mission: dev/SRE vertical loop'
  weight = 10; task_slots = 30
  parity_gate = 'Issue/alert to investigation, code/config change, approval, execution, verification, rollback capability, and signed evidence pack completes end to end without manual repair.'
  surpass_gate = 'Approval, steer/interrupt, diff/test/rollback, and evidence cards are actionable in the live workflow; rollback restores known-good state.'
  ten_out_of_ten = 'First real mission under 15 minutes; 100 percent of successful missions are verifiable with zero silent failures; cancel p95 under 250ms.'
}
$relsec = [pscustomobject]@{
  id = 'reliability_security'; label = 'Reliability and security chaos'
  weight = 10; task_slots = 30
  parity_gate = 'Duplicate requests, crash at each event-commit point, out-of-order events, disconnect/reconnect, provider timeout/429/malformed, approval timeout/denied, process-tree cancel, disk full, signature tampering, cross-tenant IDOR, sandbox escape, upgrade interruption and rollback are all handled without corruption.'
  surpass_gate = 'Failures are structured, user-visible, auditable; ledger RPO=0 RTO<60s; no double side effects.'
  ten_out_of_ten = 'Required chaos scenarios pass with zero data loss or cross-tenant leakage; recovery and evidence invariants hold under fault injection.'
}
$j.categories = @($j.categories) + @($hero, $relsec)

# 4. Write back
$json = $j | ConvertTo-Json -Depth 20
[System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
Write-Output 'manifest updated'
