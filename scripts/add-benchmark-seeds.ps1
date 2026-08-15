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
$seeds = @(
  # reliability_security chaos seeds (plan mandatory-test list)
  (New-Task 'ZAION-300-REL-001' 'reliability_security' 'Crash occurs at each event-commit point; restart must not lose or duplicate committed events' 'recovery' 'Kill the process at every ledger commit boundary and verify recovery.' 'No double side effects after restart.' 'RPO=0 and RTO under 60 seconds across all commit points.'),
  (New-Task 'ZAION-300-REL-002' 'reliability_security' 'Out-of-order or replayed events must be rejected or safely sequenced' 'idempotency' 'Deliver events out of order and duplicated; verify idempotent handling.' 'Replays return existing results; ordering invariants hold.' 'No duplicate or misordered terminal states.'),
  (New-Task 'ZAION-300-REL-003' 'reliability_security' 'Disk-full during a write must stop the commit cleanly with a user-visible error' 'recovery' 'Fill the disk mid-write and verify the operation fails cleanly.' 'Error is structured and user-visible; ledger not corrupted.' 'Disk-full scenario produces zero data loss.'),
  (New-Task 'ZAION-300-REL-004' 'reliability_security' 'Tampered signatures or receipts must be rejected' 'security' 'Alter a signed event or receipt and verify rejection.' 'Tampering is detected with no silent acceptance.' 'All tamper scenarios are rejected with audit record.'),
  (New-Task 'ZAION-300-REL-005' 'reliability_security' 'Cross-tenant IDOR attempts must be denied and audited' 'security' 'Attempt to read or write another principal state via gateway/API.' 'Cross-tenant reads and writes are zero.' 'IDOR negative suite passes with audit entries.'),
  # surface seeds
  (New-Task 'ZAION-300-TOOLS-001' 'tools' 'Write a file with read-before-edit guard and typed result' 'happy_path' 'Agent writes/edits a file and receives a typed tool result.' 'Read-before-edit and unique-match guarantees hold.' 'File tool suite passes with no unauthorized mutation.'),
  (New-Task 'ZAION-300-SESSION-001' 'session' 'Create a session, resume it, and branch without losing lineage' 'happy_path' 'Session create/resume/branch round-trip preserves signed lineage.' 'Compression lineage is preserved across branch.' 'Zero lost lineage in the session matrix.'),
  (New-Task 'ZAION-300-MEMORY-001' 'memory' 'Store and recall a memory atom with source binding' 'happy_path' 'Agent stores a memory atom and recalls it by semantic query.' 'Recall is source-bound and conflict-aware.' 'Recall@10 >= 0.90 and precision >= 0.85.'),
  (New-Task 'ZAION-300-MCP-001' 'mcp' 'Register an MCP tool and invoke it through the broker with a receipt' 'happy_path' 'MCP tool registration, invocation, and receipt join complete.' 'Receipt joins the signed proof chain.' 'MCP Inspector passes for representative servers.'),
  (New-Task 'ZAION-300-TUI-001' 'tui' 'Interactive TUI session survives protocol disruption and restores terminal' 'recovery' 'Random protocol sequences leave no corrupt state.' 'Terminal restoration is clean after crash.' 'Cancellation visible within one second.')
)
$j.tasks = @($j.tasks) + $seeds
$json = $j | ConvertTo-Json -Depth 20
[System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
Write-Output ('tasks now: ' + $j.tasks.Count)
