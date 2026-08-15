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
  # gateway depth (13 -> +5)
  @('ZAION-300-GW-007','gateway','Gateway enforces per-principal rate limits','idempotency','Per-principal limits enforced.','Over-limit rejected with audit.','Rate-limit matrix passes.'),
  @('ZAION-300-GW-008','gateway','SSE reconnect resumes the event stream','recovery','Reconnect resumes from last ack.','No missed events.','SSE resume suite passes.'),
  @('ZAION-300-GW-009','gateway','WebSocket binary frames are handled safely','security','Binary frames validated; malformed rejected.','No memory blowup.','Binary frame suite passes.'),
  @('ZAION-300-GW-010','gateway','Gateway binding to 127.0.0.1 by default','security','Default binds loopback only.','Non-loopback requires config.','Bind matrix passes.'),
  @('ZAION-300-GW-011','gateway','Request size limits are enforced','security','Oversize requests rejected.','Limit is configurable.','Size limit suite passes.'),
  # session depth
  @('ZAION-300-SES-007','session','Session handoff across channels keeps lineage','evidence','Handoff carries signed lineage.','Cross-channel continuity verifies.','Handoff lineage passes.'),
  @('ZAION-300-SES-008','session','Profile isolation prevents config bleed','security','Profile A config does not leak to B.','No cross-profile state.','Profile isolation passes.'),
  @('ZAION-300-SES-009','session','Fork a session and diverge safely','recovery','Fork diverges with separate lineage.','Both branches verifiable.','Fork suite passes.'),
  # memory depth
  @('ZAION-300-MEM-007','memory','Memory recall with atomic evidence spans','evidence','Recall answers cite atom spans.','Spans are source-bound.','Answer-span suite passes.'),
  @('ZAION-300-MEM-008','memory','Memory write requires source attribution','security','Unsourced writes rejected.','Source required.','Source-required suite passes.'),
  @('ZAION-300-MEM-009','memory','Memory size limits prevent abuse','idempotency','Oversize atoms rejected.','Limit configurable.','Memory limit suite passes.'),
  # tools depth
  @('ZAION-300-TOOLS-007','tools','Tool cancellation returns control under 250ms','recovery','Cancel p95 under 250ms.','No zombie processes.','Cancel latency suite passes.'),
  @('ZAION-300-TOOLS-008','tools','Tool result size is bounded','security','Oversize results truncated with notice.','No memory blowup.','Result bound suite passes.'),
  @('ZAION-300-TOOLS-009','tools','Delegation to sub-agent preserves policy','evidence','Sub-agent runs within parent policy.','Decisions attributable.','Delegation suite passes.'),
  # hero mission depth
  @('ZAION-300-HERO-011','hero_mission','Mission plan is reviewed before high-risk execution','approval','High-risk plan requires approval.','Denied plan aborts.','Plan approval suite passes.'),
  @('ZAION-300-HERO-012','hero_mission','Mission evidence card exports for independent verify','evidence','Evidence pack exports; independent verifier accepts.','Zero silent failures.','Evidence export passes.'),
  @('ZAION-300-HERO-013','hero_mission','Duplicate mission request returns the first result','idempotency','Same request key returns first result.','No double execution.','Mission idempotency passes.'),
  # reliability depth
  @('ZAION-300-REL-006','reliability_security','Out-of-order events are sequenced safely','recovery','Out-of-order events queue and sequence.','No corrupt state.','Ordering suite passes.'),
  @('ZAION-300-REL-007','reliability_security','Replayed committed events are ignored','idempotency','Replays return existing results.','No double side effects.','Replay suite passes.'),
  @('ZAION-300-REL-008','reliability_security','Ledger tampering is detected on load','security','Tampered ledger rejected on load.','Recovery path offered.','Ledger tamper suite passes.'),
  # channels depth
  @('ZAION-300-CH-006','channels','Webhook retry respects backoff','recovery','Retries back off; no hammering.','Deliveries settle.','Backoff suite passes.'),
  @('ZAION-300-CH-007','channels','Channel signing prevents spoofed messages','security','Unsigned messages rejected.','Signature mandatory.','Channel signing passes.'),
  @('ZAION-300-CH-008','channels','Stale channel replies are suppressed','idempotency','Newer turn owns the thread.','No stale delivery.','Stale suppression passes.'),
  # mcp/acp depth
  @('ZAION-300-MCP-004','mcp','MCP client discovers and connects to a server','happy_path','Client discovery and connection work.','Schema validated.','MCP client suite passes.'),
  @('ZAION-300-MCP-005','mcp','MCP tool streaming works for incremental tools','happy_path','Streaming tools deliver increments.','Completion is correct.','MCP streaming passes.'),
  @('ZAION-300-ACP-003','acp','ACP replay of a session works','recovery','Session replay restores state.','No corruption.','ACP replay passes.'),
  # tui depth
  @('ZAION-300-TUI-004','tui','TUI search finds past turns','happy_path','Search within TUI finds turns.','Scoped to session.','TUI search passes.'),
  @('ZAION-300-TUI-005','tui','TUI terminal restoration after crash','recovery','Terminal restored cleanly.','No leftover state.','Restoration suite passes.'),
  # skills/context depth
  @('ZAION-300-SK-004','skills','Skill rollback restores prior version','recovery','Rollback restores previous skill version.','User data preserved.','Skill rollback passes.'),
  @('ZAION-300-SK-005','skills','Skill is isolated from ambient credentials','security','Skill subprocess env filtered.','No credential leak.','Skill isolation passes.'),
  @('ZAION-300-CTX-004','context','Context assembly dedupes repeated facts','idempotency','Repeated facts deduplicated.','No stale dominance.','Context dedup passes.'),
  @('ZAION-300-CTX-005','context','Compression split keeps tool pairs intact','recovery','Paired tool calls stay together.','Forced split honest.','Tool-pair integrity passes.')
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
