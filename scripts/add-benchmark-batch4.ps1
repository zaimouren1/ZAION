$ErrorActionPreference = 'Stop'
$path = 'D:\zaion-rust\eval\benchmarks\zaion_300_v1.json'
$j = Get-Content $path -Raw | ConvertFrom-Json
function New-Task($id, $cat, $title, $type, $parity, $surpass, $ten) {
  [pscustomobject]@{
    id = $id; category = $cat; slots = 1; title = $title; status = 'planned'
    source = @{ kind = 'zaion_spec'; path = 'plans/zaion-10-10-leap-plan.md'; ref = 'main' }
    acceptance = @{ parity = @($parity); surpass = @($surpass); ten_out_of_ten = @($ten) }
    score = $null; evidence = @(); result = $null; task_type = $type
  }
}
$rows = @(
  # recovery-heavy batch (weights: recovery 15% + trust 15%)
  @('ZAION-300-REC-002','reliability_security','Interrupt a running mission and resume without double side effects','recovery','Interrupted mission resumes cleanly.','No duplicate side effects.','Resume suite has zero duplicated actions.'),
  @('ZAION-300-REC-003','reliability_security','Provider timeout mid-turn recovers the turn','recovery','Provider timeout returns structured error and retry path.','Cancellation is clean.','Timeout recovery passes within SLO.'),
  @('ZAION-300-REC-004','reliability_security','Disk-full during batch write stops cleanly','recovery','Disk-full error is structured; no corruption.','User-visible error.','Disk-full scenario has zero data loss.'),
  @('ZAION-300-REC-005','session','Session file corruption is detected and recoverable','recovery','Corrupt session file detected; recovery path offered.','No silent data loss.','Corruption recovery passes.'),
  @('ZAION-300-REC-006','gateway','Gateway restart preserves pending sessions','recovery','Restart restores pending session state.','No lost lineage.','Gateway restart suite passes.'),
  @('ZAION-300-REC-007','tools','A long tool run is cancellable mid-execution','recovery','Cancel kills the process tree and returns control.','No zombie processes.','Cancel p95 under 250ms.'),
  @('ZAION-300-REC-008','tui','TUI recovers from a terminal resize storm','recovery','Resize events do not corrupt layout or state.','Layout recovers.','Resize suite passes.'),
  @('ZAION-300-REC-009','memory','Memory store reopens after crash without loss','recovery','Reopen preserves atoms and lineage.','No corruption.','Memory crash-recovery passes.'),
  # approval-heavy
  @('ZAION-300-APR-006','tools','Deleting a file requires explicit approval','approval','Delete waits for approval; denied deletes are blocked.','Decision is audited.','Delete approval suite passes.'),
  @('ZAION-300-APR-007','gateway','Remote tool execution requires approval by default','approval','Remote exec denied without approval.','Approval is signed.','Remote exec policy passes.'),
  @('ZAION-300-APR-008','skills','Installing a third-party skill requires approval','approval','Skill install waits for approval.','Denied install is blocked.','Skill install policy passes.'),
  @('ZAION-300-APR-009','hero_mission','Approval denial aborts the mission with an audit trail','approval','Denied approval aborts; audit records the decision.','No partial execution.','Denial audit suite passes.'),
  # evidence-heavy
  @('ZAION-300-EVD-006','release','Release evidence pack verifies independently','evidence','Signed release evidence verifies offline.','Third-party verifier accepts.','Release evidence suite passes.'),
  @('ZAION-300-EVD-007','batch_eval','Benchmark result artifact is immutable and auditable','evidence','Result artifact hash is recorded and verifiable.','No post-hoc tampering.','Eval artifact suite passes.'),
  @('ZAION-300-EVD-008','mcp','MCP tool receipt joins the proof chain','evidence','Tool receipt verifies against the turn proof.','Receipt join is single.','Receipt join suite passes.'),
  @('ZAION-300-EVD-009','acp','ACP exchange records signed provenance','evidence','ACP request/response carries provenance.','Provenance verifies.','ACP provenance suite passes.'),
  # idempotency-heavy
  @('ZAION-300-IDP-006','tools','Repeated task submission returns the same result','idempotency','Same task idempotency key returns first result.','No double execution.','Idempotency suite passes.'),
  @('ZAION-300-IDP-007','gateway','Webhook duplicate delivery is deduplicated','idempotency','Duplicate webhook delivery processes once.','Receipt join is single.','Webhook dedup passes.'),
  @('ZAION-300-IDP-008','memory','Repeated memory write is idempotent','idempotency','Same atom write produces single atom.','No duplicates.','Memory write idempotency passes.'),
  @('ZAION-300-IDP-009','session','Resume with a replayed event does not duplicate state','idempotency','Replayed event is ignored after commit.','State is exact-once.','Event replay suite passes.'),
  # happy_path depth for thin categories
  @('ZAION-300-SK-002','skills','Discover and inspect a skill before install','happy_path','Skill discovery and inspection work.','Metadata is accurate.','Skill discovery suite passes.'),
  @('ZAION-300-SK-003','skills','Disable and re-enable a skill','happy_path','Disable/enable round-trips cleanly.','No state loss.','Skill toggle suite passes.'),
  @('ZAION-300-ONB-002','onboarding','First answer completes under three minutes on a clean machine','happy_path','Clean-machine first answer succeeds.','Median under three minutes.','First-answer matrix at 95 percent.'),
  @('ZAION-300-ONB-003','onboarding','Diagnose a provider configuration error with guidance','happy_path','Error is diagnosed with actionable guidance.','No manual state repair.','Diagnosis suite passes.'),
  @('ZAION-300-BE-002','batch_eval','Batch rerun produces identical results','idempotency','Same input reruns produce identical outputs.','Rerun is reproducible.','Reproducibility suite passes.'),
  @('ZAION-300-BE-003','batch_eval','Batch run recovers from a mid-run failure','recovery','Mid-run failure resumes remaining tasks.','No lost results.','Batch recovery passes.'),
  @('ZAION-300-REL-002','release','Upgrade preserves user data and settings','recovery','Upgrade keeps config and state.','Rollback restores prior version.','Upgrade suite passes.'),
  @('ZAION-300-REL-003','release','Uninstall removes all artifacts cleanly','happy_path','Uninstall removes data and config cleanly.','No leftovers.','Uninstall suite passes.'),
  @('ZAION-300-COM-002','community','Contributor can build from source with documented steps','happy_path','Source build works from clean checkout.','Docs match reality.','Build-from-source passes.'),
  @('ZAION-300-COM-003','community','Issue report includes required diagnostics','happy_path','Diagnostics gather works.','Template is complete.','Issue diagnostics pass.'),
  # tool/session/gateway depth
  @('ZAION-300-TOOLS-005','tools','Read a file outside workspace is denied by default','security','Out-of-workspace read is denied.','Denial is audited.','Path policy suite passes.'),
  @('ZAION-300-TOOLS-006','tools','Tool output with secrets is redacted','security','Secrets in tool output are redacted.','No secret in logs.','Redaction suite passes.'),
  @('ZAION-300-SES-005','session','Session branch preserves parent lineage','evidence','Branch inherits signed lineage.','Independent verification accepts.','Branch lineage passes.'),
  @('ZAION-300-SES-006','session','Session prune keeps evidence trail','recovery','Pruned sessions keep proof trail.','No lineage loss.','Prune suite passes.'),
  @('ZAION-300-GW-005','gateway','Gateway rejects malformed frames','security','Malformed frames rejected with structured error.','No state corruption.','Framing suite passes.'),
  @('ZAION-300-GW-006','gateway','Gateway write audit logs every mutation','evidence','Every write has an audit entry.','Audit is complete.','Write audit suite passes.'),
  @('ZAION-300-CH-004','channels','Channel outbound delivery has a receipt','evidence','Delivery receipt is recorded.','Receipt verifies.','Channel receipt suite passes.'),
  @('ZAION-300-CH-005','channels','Channel message over size limit is rejected','security','Oversize message rejected with clear error.','No truncation surprise.','Size limit suite passes.'),
  @('ZAION-300-MEM-005','memory','Memory conflict is surfaced to the user','recovery','Conflicting atoms are surfaced.','User resolves conflict.','Conflict suite passes.'),
  @('ZAION-300-MEM-006','memory','Memory expiry prevents stale recall','idempotency','Expired atoms are not recalled.','Expiry propagates.','Expiry suite passes.'),
  @('ZAION-300-HERO-009','hero_mission','Full hero loop: alert, fix, approve, execute, verify, evidence','evidence','End-to-end mission produces signed evidence pack.','Independently verifiable.','Hero loop completes under 15 minutes.'),
  @('ZAION-300-HERO-010','hero_mission','Rollback after a regression restores known-good','recovery','Regression triggers rollback.','Known-good restored with evidence.','Rollback suite passes.')
)
$used = @{}
foreach ($t in $j.tasks) { $used[$t.id] = $true }
$added = 0
foreach ($row in $rows) {
  if ($used.ContainsKey($row[0])) { continue }
  $j.tasks += New-Task $row[0] $row[1] $row[2] $row[3] $row[4] $row[5] $row[6]
  $used[$row[0]] = $true
  $added++
}
$json = $j | ConvertTo-Json -Depth 20
[System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
Write-Output ('added: ' + $added + ' | total: ' + $j.tasks.Count)
