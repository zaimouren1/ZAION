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
  @('ZAION-300-CTX-006','context','Context truncation keeps the task instruction','recovery','Task instruction survives truncation.','No critical loss.','Truncation suite passes.'),
  @('ZAION-300-CTX-007','context','Provider limit errors retry with backoff','recovery','429/limit errors retry.','No lost turn.','Limit retry passes.'),
  @('ZAION-300-CTX-008','context','Context assembly marks provenance per block','evidence','Each context block has provenance.','Provenance verifies.','Context provenance passes.'),
  @('ZAION-300-ACP-004','acp','ACP stream closing frees resources','recovery','Stream close releases resources.','No leak.','ACP lifecycle passes.'),
  @('ZAION-300-ACP-005','acp','ACP protocol version negotiation','happy_path','Version negotiation succeeds.','Fallback works.','Version matrix passes.'),
  @('ZAION-300-ENV-004','environments','Environment teardown cleans artifacts','recovery','Teardown removes temp state.','No leftovers.','Teardown suite passes.'),
  @('ZAION-300-ENV-005','environments','Environment identity is unique per run','evidence','Each run has unique identity.','Identity verifies.','Identity uniqueness passes.'),
  @('ZAION-300-ONB-004','onboarding','Provider key entry validates format','security','Invalid keys rejected with guidance.','No key in logs.','Key validation passes.'),
  @('ZAION-300-ONB-005','onboarding','First run creates a signed principal key','evidence','Principal key created with backup guidance.','Key continuity verifies.','Key setup passes.'),
  @('ZAION-300-REL-004','release','Release pipeline produces reproducible builds','evidence','Same commit builds identical artifact.','Reproducibility record exists.','Reproducible build passes.'),
  @('ZAION-300-REL-005','release','Rollback drill restores the prior release','recovery','Rollback restores prior version.','Evidence recorded.','Rollback drill passes.'),
  @('ZAION-300-COM-004','community','Localization strings are complete','happy_path','UI strings have all locales.','Fallback works.','i18n suite passes.'),
  @('ZAION-300-SK-006','skills','Skill manifest validates before install','security','Invalid manifest rejected.','Signed provenance required.','Skill manifest passes.'),
  @('ZAION-300-SK-007','skills','Skill invocation receives scoped permissions','approval','Skill runs with declared scopes only.','Denials audited.','Skill scope passes.'),
  @('ZAION-300-TUI-006','tui','TUI copes with non-UTF8 output','recovery','Non-UTF8 output sanitized.','No crash.','Encoding suite passes.'),
  @('ZAION-300-TUI-007','tui','TUI approval shows risk context','approval','Approval prompt shows risk details.','Decision informed.','Risk display passes.'),
  @('ZAION-300-BE-004','batch_eval','Batch run enforces per-task budget','idempotency','Per-task token budget enforced.','No runaway.','Budget suite passes.'),
  @('ZAION-300-BE-005','batch_eval','Batch report links each score to evidence','evidence','Score has evidence reference.','Evidence immutable.','Evidence link passes.'),
  @('ZAION-300-MCP-006','mcp','MCP server crash does not take down the agent','recovery','Server failure isolated.','Reconnect offered.','MCP isolation passes.'),
  @('ZAION-300-MCP-007','mcp','MCP tool name collisions are resolved safely','security','Collision rejected or namespaced.','No shadowing.','MCP collision passes.'),
  @('ZAION-300-MCP-008','mcp','MCP client timeout is bounded','recovery','Timeout returns structured error.','No hang.','MCP timeout passes.'),
  @('ZAION-300-CH-009','channels','Channel delivery is idempotent per message id','idempotency','Same message id delivered once.','Receipt join single.','Channel idempotency passes.'),
  @('ZAION-300-CH-010','channels','Channel receives size-limited media','security','Oversize media rejected.','No memory blowup.','Media limit passes.'),
  @('ZAION-300-MEM-010','memory','Memory store compaction preserves atoms','recovery','Compaction keeps all atoms.','Lineage intact.','Compaction passes.'),
  @('ZAION-300-MEM-011','memory','Memory export is portable','happy_path','Export/import round-trips atoms.','Schema validated.','Memory export passes.'),
  @('ZAION-300-MEM-012','memory','Memory write conflict is detected','idempotency','Conflicting write flagged.','User resolves.','Write conflict passes.'),
  @('ZAION-300-TOOLS-010','tools','Tool environment variables are filtered','security','Agent tools see filtered env.','No credential leak.','Env filter passes.'),
  @('ZAION-300-TOOLS-011','tools','Tool retry on transient failure','recovery','Transient failure retries.','No duplicate side effect.','Tool retry passes.'),
  @('ZAION-300-TOOLS-012','tools','Tool approval state is persisted','approval','Approval survives restart.','No re-request.','Approval persist passes.'),
  @('ZAION-300-TOOLS-013','tools','Tool output schema is validated','evidence','Tool result matches manifest schema.','Type-checked.','Schema suite passes.'),
  @('ZAION-300-SES-010','session','Session export redacts secrets','security','Export omits secrets.','No secret in artifact.','Redaction passes.'),
  @('ZAION-300-SES-011','session','Session search ranks by relevance','happy_path','Search returns relevant results first.','Scoped to principal.','Search rank passes.'),
  @('ZAION-300-SES-012','session','Session resume after process restart','recovery','Restart restores session.','No lost context.','Resume passes.'),
  @('ZAION-300-SES-013','session','Session delete is confirmed','approval','Delete requires confirmation.','Audit recorded.','Delete confirm passes.'),
  @('ZAION-300-SES-014','session','Session branch conflict is resolved','recovery','Branch conflict resolved with prompt.','No silent overwrite.','Branch conflict passes.'),
  @('ZAION-300-SES-015','session','Session timeline is auditable','evidence','Every transition has a record.','Timeline verifies.','Timeline audit passes.'),
  @('ZAION-300-GW-012','gateway','Gateway CORS policy is restrictive by default','security','Default CORS denies cross-origin.','Config explicit.','CORS matrix passes.'),
  @('ZAION-300-GW-013','gateway','Gateway CSRF protection for state changes','security','State changes require CSRF token.','No forged change.','CSRF suite passes.'),
  @('ZAION-300-GW-014','gateway','Gateway health endpoint reveals no internals','security','Health hides internals.','No version leak.','Health endpoint passes.'),
  @('ZAION-300-GW-015','gateway','Gateway session token rotation','security','Tokens rotate; old tokens expire.','No reuse.','Rotation suite passes.'),
  @('ZAION-300-GW-016','gateway','Gateway request tracing end to end','evidence','Request carries trace id through.','Trace joins evidence.','Tracing passes.'),
  @('ZAION-300-GW-017','gateway','Gateway graceful shutdown drains connections','recovery','Shutdown drains in-flight requests.','No dropped state.','Graceful shutdown passes.'),
  @('ZAION-300-GW-018','gateway','Gateway connection limits prevent exhaustion','security','Max connections enforced.','Over-limit rejected.','Connection limit passes.'),
  @('ZAION-300-HERO-014','hero_mission','Mission runs to completion without human rework','happy_path','Mission completes first try.','No rework.','First-try success suite passes.'),
  @('ZAION-300-HERO-015','hero_mission','Mission evidence survives process crash','recovery','Evidence persisted before crash.','Verifiable after restart.','Evidence durability passes.'),
  @('ZAION-300-HERO-016','hero_mission','Mission approval chain records all decisions','evidence','Decision chain complete and signed.','Independent verify accepts.','Decision chain passes.'),
  @('ZAION-300-HERO-017','hero_mission','Mission plan diff is reviewable before execute','approval','Plan diff shown before execution.','No blind execution.','Diff review passes.'),
  @('ZAION-300-HERO-018','hero_mission','Mission side effects are reversible','recovery','Executed change is reversible.','Rollback restores.','Reversibility passes.'),
  @('ZAION-300-REL-009','reliability_security','Provider 429 storm is handled with backoff','recovery','429s back off; no crash.','Queue bounded.','429 storm passes.'),
  @('ZAION-300-REL-010','reliability_security','Signature verification failure is user-visible','security','Bad signature produces clear error.','No silent accept.','Signature fail passes.'),
  @('ZAION-300-REL-011','reliability_security','Upgrade interruption recovers','recovery','Interrupted upgrade resumes or rolls back.','No corrupt install.','Upgrade recovery passes.')
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
