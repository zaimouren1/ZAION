$ErrorActionPreference = 'Stop'
$path = 'D:\zaion-rust\eval\benchmarks\zaion_300_v1.json'
$j = Get-Content $path -Raw | ConvertFrom-Json

function New-Task($id, $cat, $title, $type, $parity, $surpass, $ten, $src) {
  $p = if ($src -eq 'zaion_spec') { 'plans/zaion-10-10-leap-plan.md' } else { 'crates/zaion-cli/src/commands/system.rs' }
  [pscustomobject]@{
    id = $id; category = $cat; slots = 1; title = $title; status = 'planned'
    source = @{ kind = $src; path = $p; ref = 'main' }
    acceptance = @{ parity = @($parity); surpass = @($surpass); ten_out_of_ten = @($ten) }
    score = $null; evidence = @(); result = $null; task_type = $type
  }
}
$rows = @(
  @('ZAION-300-SES-001','session','Search past sessions by content and resume with lineage','happy_path','Session search returns matches; resume restores context.','Search is scoped to the principal.','Ten-thousand-session search is correct and isolated.'),
  @('ZAION-300-SES-002','session','Export a session as a portable artifact and re-import it','happy_path','Export/import round-trips session state.','Export is signed and verifiable.','Export/import matrix passes with no lineage loss.'),
  @('ZAION-300-SES-003','session','Reset a session without losing the ledger trail','idempotency','Reset clears session state; ledger trail remains.','Reset is reversible via ledger.','Reset preserves evidence lineage.'),
  @('ZAION-300-SES-004','session','Two sessions under one principal never cross state','security','Parallel sessions keep isolated state.','No cross-session leakage.','Multi-session isolation suite passes.'),
  @('ZAION-300-MEM-002','memory','Invalidate a memory atom after source change','recovery','Source change invalidates dependent atoms.','Invalidation propagates to recall.','Expiry/invalidation tests pass.'),
  @('ZAION-300-MEM-003','memory','Memory recall excludes another principal atoms','security','Query only returns the principal own atoms.','No cross-principal contamination.','Recall isolation passes.'),
  @('ZAION-300-MEM-004','memory','Prefetch relevant memory before a turn','happy_path','Turn starts with prefetched context.','Prefetch is source-bound.','Prefetch improves recall quality.'),
  @('ZAION-300-CTX-001','context','Automatic compression fires and preserves tool pairs','recovery','Compression split keeps paired tool calls intact.','Forced split is honest.','Tool-pair integrity passes.'),
  @('ZAION-300-CTX-002','context','Context budget is respected across a long session','happy_path','Turn stays within token budget.','Budget is configurable.','Budget enforcement passes.'),
  @('ZAION-300-CTX-003','context','Compressed child session lineage verifies to parent','evidence','Child session proof chains to parent.','Independent verification accepts chain.','Lineage verification passes.'),
  @('ZAION-300-GW-001','gateway','SSE stream survives client reconnect','recovery','Client reconnects and resumes the stream.','Backlog replay is correct.','SSE reconnect suite passes.'),
  @('ZAION-300-GW-002','gateway','WebSocket upgrade is authenticated','security','Unauthenticated upgrade rejected.','Token required for WS.','WS auth negatives pass.'),
  @('ZAION-300-GW-003','gateway','Rate limiting blocks abusive requests','idempotency','Over-limit requests rejected with audit.','Limits are per-principal.','Rate-limit suite passes.'),
  @('ZAION-300-GW-004','gateway','Loopback mode uses generated credentials','security','Local gateway uses ephemeral credentials.','Non-loopback requires real auth.','Bind/auth matrix passes.'),
  @('ZAION-300-CH-001','channels','Telegram command round-trips with typing and reaction','happy_path','Message flows with typing indicator and reaction.','Reaction is recorded.','Channel live smoke passes.'),
  @('ZAION-300-CH-002','channels','Webhook delivery retries on transient failure','recovery','Transient failure retries with backoff.','No duplicate delivery.','Webhook retry suite passes.'),
  @('ZAION-300-CH-003','channels','Channel message respects topic routing','happy_path','Messages route to correct thread/topic.','Routing is principal-bound.','Topic routing matrix passes.'),
  @('ZAION-300-ENV-001','environments','Run a mission in a container and verify isolation','security','Container env isolates host state.','Path isolation and cleanup.','Container suite passes.'),
  @('ZAION-300-ENV-002','environments','Environment identity is bound to the session','evidence','Env identity appears in receipts.','Identity is verifiable.','Env identity suite passes.'),
  @('ZAION-300-ACP-001','acp','ACP client negotiation and capability exchange','happy_path','Client and server negotiate capabilities.','Compatibility across clients.','ACP client suite passes.'),
  @('ZAION-300-ACP-002','acp','ACP request/response with permission scoping','approval','Scoped permissions enforced per request.','Denials are audited.','ACP permission suite passes.'),
  @('ZAION-300-MCP-002','mcp','MCP server tool list is scoped by policy','security','Policy filters exposed tools.','Per-server policy works.','MCP policy suite passes.'),
  @('ZAION-300-MCP-003','mcp','MCP tool failure returns structured error','recovery','Tool error is structured and user-visible.','No crash propagates.','MCP error suite passes.'),
  @('ZAION-300-TUI-002','tui','TUI queue shows pending turns and steers','happy_path','Queue and steer work interactively.','Steer is visible.','TUI queue suite passes.'),
  @('ZAION-300-TUI-003','tui','TUI approval prompt renders and handles decision','approval','Approval prompt appears and decision routes.','Decision is signed.','TUI approval suite passes.'),
  @('ZAION-300-TOOLS-002','tools','Shell command runs with timeout and typed result','happy_path','Shell tool enforces timeout and returns typed result.','Timeout kills process tree.','Shell suite passes.'),
  @('ZAION-300-TOOLS-003','tools','Browser tool navigates and extracts content','happy_path','Browser navigation and extraction work.','No secret leak.','Browser suite passes.'),
  @('ZAION-300-TOOLS-004','tools','Network tool is denied by default policy','security','Network access requires explicit grant.','Denial is audited.','Network policy suite passes.'),
  @('ZAION-300-SK-001','skills','Skill update preserves user data','recovery','Update keeps user state.','Rollback restores old version.','Skill lifecycle suite passes.'),
  @('ZAION-300-REL-001','release','Release candidate verifies checksums and signatures','evidence','Artifact checksum and signature verify.','SBOM present.','Release gate passes.'),
  @('ZAION-300-HERO-006','hero_mission','Investigate a production alert end to end and document root cause','happy_path','Alert triage produces root-cause writeup with evidence.','Writeup is auditable.','Root-cause mission under 10 minutes.')
)
$used = @{}
foreach ($t in $j.tasks) { $used[$t.id] = $true }
$added = 0
foreach ($row in $rows) {
  if ($used.ContainsKey($row[0])) { continue }
  $j.tasks += New-Task $row[0] $row[1] $row[2] $row[3] $row[4] $row[5] $row[6] 'zaion_spec'
  $used[$row[0]] = $true
  $added++
}
$json = $j | ConvertTo-Json -Depth 20
[System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
Write-Output ('added: ' + $added + ' | total tasks: ' + $j.tasks.Count)
