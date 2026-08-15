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
  # hero +6 (to ~24)
  @('ZAION-300-HERO-019','hero_mission','Mission accepted by user with explicit consent','approval','Mission waits for explicit acceptance.','Acceptance is recorded.','Consent suite passes.'),
  @('ZAION-300-HERO-020','hero_mission','Mission executes real actions with full audit','evidence','Real actions have audit entries.','Audit verifies.','Action audit passes.'),
  @('ZAION-300-HERO-021','hero_mission','Mission plan steers mid-execution','recovery','Steer adjusts the plan.','No corrupt state.','Steer suite passes.'),
  @('ZAION-300-HERO-022','hero_mission','Mission completes and is not rolled back in 24h','happy_path','Completed mission stable for 24h.','No regression.','Weekly accepted missions grow.'),
  @('ZAION-300-HERO-023','hero_mission','Mission cost is estimated before execution','approval','Cost estimate shown before execute.','Budget respected.','Cost preview passes.'),
  @('ZAION-300-HERO-024','hero_mission','Mission respects workspace boundary','security','Workspace escape blocked.','No path escape.','Boundary suite passes.'),
  # relsec +6 (to ~20)
  @('ZAION-300-REL-012','reliability_security','Process tree cancel leaves no orphans','recovery','Cancel kills the full tree.','No zombies.','Tree cancel passes.'),
  @('ZAION-300-REL-013','reliability_security','Sandbox escape attempt is blocked','security','Escape attempt denied.','Audit recorded.','Sandbox escape suite passes.'),
  @('ZAION-300-REL-014','reliability_security','Ledger append-only invariant holds','evidence','No reordering/truncation possible.','Chain verifies.','Ledger invariant passes.'),
  @('ZAION-300-REL-015','reliability_security','Concurrent turns do not interleave state','idempotency','Concurrent turns isolated.','No interleaving.','Concurrency suite passes.'),
  @('ZAION-300-REL-016','reliability_security','Provider malformed response is handled','recovery','Malformed response produces structured error.','No crash.','Malformed suite passes.'),
  @('ZAION-300-REL-017','reliability_security','Memory exhaustion is bounded','recovery','OOM scenarios bounded.','No process death.','Memory bound passes.'),
  # gateway +6 (to ~24)
  @('ZAION-300-GW-019','gateway','Gateway token auth for non-loopback','security','Non-loopback requires token.','No anonymous writes.','Non-loopback auth passes.'),
  @('ZAION-300-GW-020','gateway','Gateway TLS termination supported','security','TLS works for external.','Cert rotation works.','TLS suite passes.'),
  @('ZAION-300-GW-021','gateway','Gateway routes by principal','happy_path','Requests route to principal state.','No cross-tenant.','Routing passes.'),
  @('ZAION-300-GW-022','gateway','Gateway SSE keepsalive','recovery','Keepalive prevents idle drop.','Reconnect works.','Keepalive passes.'),
  @('ZAION-300-GW-023','gateway','Gateway version endpoint is safe','happy_path','Version endpoint works.','No internals.','Version endpoint passes.'),
  @('ZAION-300-GW-024','gateway','Gateway config reload without restart','recovery','Config reload applies live.','No dropped state.','Reload suite passes.'),
  # session +6 (to ~21)
  @('ZAION-300-SES-016','session','Session ownership transfer requires approval','approval','Transfer approved and signed.','Audit recorded.','Transfer suite passes.'),
  @('ZAION-300-SES-017','session','Session archive and restore','recovery','Archive restores fully.','Lineage intact.','Archive suite passes.'),
  @('ZAION-300-SES-018','session','Session metadata is searchable','happy_path','Metadata search works.','Scoped.','Metadata search passes.'),
  @('ZAION-300-SES-019','session','Session timeout locks state','security','Idle session locks.','Resume requires unlock.','Timeout lock passes.'),
  @('ZAION-300-SES-020','session','Session compression is reversible','recovery','Compressed session restores.','No data loss.','Compression reverse passes.'),
  @('ZAION-300-SES-021','session','Session diff shows changes','evidence','Diff of session state available.','Changes attributable.','Diff suite passes.'),
  # tools +6 (to ~19)
  @('ZAION-300-TOOLS-014','tools','Tool chaining preserves policy context','approval','Chained tools keep policy.','No escalation.','Chain policy passes.'),
  @('ZAION-300-TOOLS-015','tools','Tool result hashed for tamper detection','evidence','Result hash verifies.','No tamper.','Result hash passes.'),
  @('ZAION-300-TOOLS-016','tools','Tool concurrent execution is serialized safely','idempotency','Concurrent calls ordered.','No interleaving.','Serialization passes.'),
  @('ZAION-300-TOOLS-017','tools','Tool plugin sandbox isolates host','security','Plugin cannot escape sandbox.','No host access.','Plugin sandbox passes.'),
  @('ZAION-300-TOOLS-018','tools','Tool timeout is configurable','recovery','Timeout config applies.','Kill tree.','Timeout config passes.'),
  @('ZAION-300-TOOLS-019','tools','Tool error surfaces to user','happy_path','Errors are user-visible.','No silent failure.','Error surfacing passes.'),
  # memory +6 (to ~18)
  @('ZAION-300-MEM-013','memory','Memory recall is fast under load','happy_path','Recall latency bounded.','No degradation.','Recall perf passes.'),
  @('ZAION-300-MEM-014','memory','Memory atom update preserves history','evidence','Atom history chain kept.','Update verifiable.','Atom history passes.'),
  @('ZAION-300-MEM-015','memory','Memory store is encrypted at rest','security','At-rest encryption works.','Key rotation works.','At-rest suite passes.'),
  @('ZAION-300-MEM-016','memory','Memory shutdown flushes pending writes','recovery','Shutdown flushes atomically.','No loss.','Shutdown flush passes.'),
  @('ZAION-300-MEM-017','memory','Memory recall dedupes repeated answers','idempotency','Repeated recall returns stable answer.','No duplicates.','Recall dedup passes.'),
  @('ZAION-300-MEM-018','memory','Memory governance lets user delete atoms','approval','User delete confirmed.','Deletion propagates.','Governance suite passes.'),
  # channels +6 (to ~16)
  @('ZAION-300-CH-011','channels','Channel offline queueing delivers later','recovery','Offline messages queued.','Delivery on reconnect.','Offline queue passes.'),
  @('ZAION-300-CH-012','channels','Channel command parsing is strict','security','Malformed commands rejected.','No injection.','Command parse passes.'),
  @('ZAION-300-CH-013','channels','Channel multi-tenant isolation','security','Tenant A messages not seen by B.','No leakage.','Tenant isolation passes.'),
  @('ZAION-300-CH-014','channels','Channel typing indicator lifecycle','happy_path','Typing shows and clears.','No stuck indicator.','Typing suite passes.'),
  @('ZAION-300-CH-015','channels','Channel reaction confirms completion','happy_path','Reaction confirms delivery.','Recorded.','Reaction suite passes.'),
  @('ZAION-300-CH-016','channels','Channel webhook auth required','security','Webhook requires signature.','No spoofed delivery.','Webhook auth passes.'),
  # mcp +5 (to ~13)
  @('ZAION-300-MCP-009','mcp','MCP server list refresh','happy_path','Server list updates.','No stale entries.','List refresh passes.'),
  @('ZAION-300-MCP-010','mcp','MCP tool annotation affects display','happy_path','Annotations render.','Metadata accurate.','Annotation suite passes.'),
  @('ZAION-300-MCP-011','mcp','MCP connection pooling','recovery','Pool reuses connections.','No leak.','Pooling passes.'),
  @('ZAION-300-MCP-012','mcp','MCP request has request id trace','evidence','Request id flows through.','Trace joins.','Request id passes.'),
  @('ZAION-300-MCP-013','mcp','MCP server authorization scope','approval','Server granted scoped access.','Denials audited.','Server scope passes.'),
  # context +5 (to ~13)
  @('ZAION-300-CTX-009','context','Context budget scales with model','happy_path','Budget adapts to model limits.','No overflow.','Budget scaling passes.'),
  @('ZAION-300-CTX-010','context','Context sensitive data minimized','security','Secrets excluded from context.','Redaction applies.','Minimization passes.'),
  @('ZAION-300-CTX-011','context','Context assembly order is stable','idempotency','Same input same order.','Deterministic.','Order stability passes.'),
  @('ZAION-300-CTX-012','context','Context compression preserves instructions','recovery','Instructions survive compression.','No instruction loss.','Instruction preservation passes.'),
  @('ZAION-300-CTX-013','context','Context eviction policy is configurable','happy_path','Eviction policy applies.','Configurable.','Eviction passes.'),
  # tui +4 (to ~11)
  @('ZAION-300-TUI-008','tui','TUI multiplexes multiple sessions','happy_path','Session switcher works.','State isolated.','Multiplex passes.'),
  @('ZAION-300-TUI-009','tui','TUI color scheme accessible','happy_path','Accessible contrast.','Theme switchable.','Accessibility passes.'),
  @('ZAION-300-TUI-010','tui','TUI streaming pauses on scroll','recovery','Scroll pause holds stream.','Resume works.','Scroll pause passes.'),
  @('ZAION-300-TUI-011','tui','TUI command palette','happy_path','Command palette works.','Discoverable.','Palette passes.'),
  # acp +4 (to ~9)
  @('ZAION-300-ACP-006','acp','ACP request cancellation','recovery','Cancel mid-request works.','No stale reply.','ACP cancel passes.'),
  @('ZAION-300-ACP-007','acp','ACP permission scoping per request','approval','Per-request scopes enforced.','Denials audited.','ACP scope passes.'),
  @('ZAION-300-ACP-008','acp','ACP session resume','recovery','Session resumes.','No context loss.','ACP resume passes.'),
  @('ZAION-300-ACP-009','acp','ACP error handling','happy_path','Errors structured.','No crash.','ACP errors pass.'),
  # skills +4 (to ~11)
  @('ZAION-300-SK-008','skills','Skill version pinning','idempotency','Pinned version stable.','Upgrade opt-in.','Version pin passes.'),
  @('ZAION-300-SK-009','skills','Skill compatibility check','happy_path','Incompatible skill flagged.','Guidance offered.','Compatibility passes.'),
  @('ZAION-300-SK-010','skills','Skill evaluation before install','approval','Evaluation gates install.','No blind install.','Skill eval passes.'),
  @('ZAION-300-SK-011','skills','Skill uninstall cleans state','recovery','Uninstall removes state.','No leftovers.','Uninstall passes.'),
  # onboarding +4 (to ~8)
  @('ZAION-300-ONB-006','onboarding','Config migration preserves user data','recovery','Migration keeps data.','No loss.','Migration passes.'),
  @('ZAION-300-ONB-007','onboarding','Uninstall removes config cleanly','happy_path','Clean uninstall.','No leftovers.','Uninstall passes.'),
  @('ZAION-300-ONB-008','onboarding','Offline install path works','happy_path','Offline install supported.','No network needed.','Offline install passes.'),
  @('ZAION-300-ONB-009','onboarding','Setup detects conflicting config','security','Conflict detected.','Guidance offered.','Conflict detect passes.'),
  # batch_eval +4 (to ~9)
  @('ZAION-300-BE-006','batch_eval','Batch run isolation between tasks','security','Task A cannot affect B.','No bleed.','Isolation passes.'),
  @('ZAION-300-BE-007','batch_eval','Batch progress is observable','happy_path','Progress updates visible.','Resumable.','Progress passes.'),
  @('ZAION-300-BE-008','batch_eval','Batch failure does not stop the run','recovery','Failed task recorded; run continues.','Report complete.','Failure isolation passes.'),
  @('ZAION-300-BE-009','batch_eval','Batch budget shared across tasks','idempotency','Shared budget enforced.','No runaway.','Shared budget passes.'),
  # release +4 (to ~9)
  @('ZAION-300-REL-006','release','Release changelog is complete','happy_path','Changelog reflects changes.','Links to commits.','Changelog passes.'),
  @('ZAION-300-REL-007','release','Release smoke test passes','happy_path','Smoke suite green.','Fast feedback.','Smoke passes.'),
  @('ZAION-300-REL-008','release','Release artifacts have SBOM','evidence','SBOM generated and verified.','Dependencies listed.','SBOM passes.'),
  @('ZAION-300-REL-009','release','Release rollback is tested','recovery','Rollback drill passes.','Evidence recorded.','Rollback test passes.'),
  # environments +4 (to ~9)
  @('ZAION-300-ENV-006','environments','Environment network is restricted','security','Deny-by-default network.','Grants explicit.','Network policy passes.'),
  @('ZAION-300-ENV-007','environments','Environment resource limits','recovery','CPU/mem limits enforced.','No host impact.','Resource limits pass.'),
  @('ZAION-300-ENV-008','environments','Environment snapshot and restore','recovery','Snapshot restores.','No data loss.','Snapshot passes.'),
  @('ZAION-300-ENV-009','environments','Environment config is declarative','happy_path','Declarative config works.','Reproducible.','Declarative passes.'),
  # community +4 (to ~8)
  @('ZAION-300-COM-005','community','Docs searchable','happy_path','Docs search works.','Current.','Docs search passes.'),
  @('ZAION-300-COM-006','community','Issue templates enforced','happy_path','Templates guide reports.','Complete info.','Issue templates pass.'),
  @('ZAION-300-COM-007','community','Community metrics visible','happy_path','Metrics dashboard works.','Honest.','Metrics pass.'),
  @('ZAION-300-COM-008','community','Security reporting channel works','security','Vulnerability report path works.','Disclosure policy.','Security channel passes.')
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
