# Run the executable benchmark suite with sample executors (honest baseline: samples do not solve tasks except channel flow)
$ErrorActionPreference = "Continue"
$H = "D:/zaion-rust/eval/harness/runner.py"
$SAMPLE = "python D:/zaion-rust/eval/harness/sample_executor.py"
  $SECOP = "python D:/zaion-rust/eval/harness/sample_security_executor.py"
  $HEROOP = "python D:/zaion-rust/eval/harness/sample_hero_executor.py"
  $SREOP = "python D:/zaion-rust/eval/harness/sample_sre_executor.py"
  $RECOP = "python D:/zaion-rust/eval/harness/sample_recovery_executor.py"
$CHANNEL = "python D:/zaion-rust/eval/harness/sample_channel_executor.py"
  $FILEOP = "python D:/zaion-rust/eval/harness/sample_file_executor.py"
  $MEMOP = "python D:/zaion-rust/eval/harness/sample_memory_executor.py"
  $SESOP = "python D:/zaion-rust/eval/harness/sample_session_executor.py"
  $IDPOP = "python D:/zaion-rust/eval/harness/sample_idempotency_executor.py"
  $APROP = "python D:/zaion-rust/eval/harness/sample_approval_executor.py"
  $EVDOP = "python D:/zaion-rust/eval/harness/sample_evidence_executor.py"
  $SKOP = "python D:/zaion-rust/eval/harness/sample_skill_executor.py"
  $CTXOP = "python D:/zaion-rust/eval/harness/sample_context_executor.py"
  $ONBOP = "python D:/zaion-rust/eval/harness/sample_onboarding_executor.py"
  $RELOP = "python D:/zaion-rust/eval/harness/sample_release_executor.py"
  $BEOP = "python D:/zaion-rust/eval/harness/sample_batch_executor.py"
  $GWOP = "python D:/zaion-rust/eval/harness/sample_gateway_executor.py"
  $ENVOP = "python D:/zaion-rust/eval/harness/sample_env_executor.py"
  $MCPOP = "python D:/zaion-rust/eval/harness/sample_mcp_executor.py"
  $ACPOP = "python D:/zaion-rust/eval/harness/sample_acp_executor.py"
  $UIOP = "python D:/zaion-rust/eval/harness/sample_ui_executor.py"
  $R2OP = "python D:/zaion-rust/eval/harness/sample_rel002_executor.py"
  $A2OP = "python D:/zaion-rust/eval/harness/sample_acp002_executor.py"
  $B2OP = "python D:/zaion-rust/eval/harness/sample_be002_executor.py"
  $M2OP = "python D:/zaion-rust/eval/harness/sample_mem002_executor.py"
  $S2OP = "python D:/zaion-rust/eval/harness/sample_ses002_executor.py"
  $C1OP = "python D:/zaion-rust/eval/harness/sample_ctx001_executor.py"
  $S1OP = "python D:/zaion-rust/eval/harness/sample_sec001_executor.py"
  $S2KOP = "python D:/zaion-rust/eval/harness/sample_sk002_executor.py"
  $G2OP = "python D:/zaion-rust/eval/harness/sample_gw002_executor.py"
  $E3OP = "python D:/zaion-rust/eval/harness/sample_env003_executor.py"
  $M2P2OP = "python D:/zaion-rust/eval/harness/sample_mcp002_executor.py"
  $SEC4OP = "python D:/zaion-rust/eval/harness/sample_sec004_executor.py"
  $B3OP = "python D:/zaion-rust/eval/harness/sample_be003_executor.py"
  $RBOP = "python D:/zaion-rust/eval/harness/sample_rollback_executor.py"
  $E5OP = "python D:/zaion-rust/eval/harness/sample_env005_executor.py"
  $E6OP = "python D:/zaion-rust/eval/harness/sample_env006_executor.py"
  $H5OP = "python D:/zaion-rust/eval/harness/sample_hero005_executor.py"
  $H6OP = "python D:/zaion-rust/eval/harness/sample_hero006_executor.py"
  $H11OP = "python D:/zaion-rust/eval/harness/sample_hero011_executor.py"
  $T2OP = "python D:/zaion-rust/eval/harness/sample_tui002_executor.py"
  $H2OP = "python D:/zaion-rust/eval/harness/sample_hero002_executor.py"
  $C3OP = "python D:/zaion-rust/eval/harness/sample_ctx003_executor.py"
  $M3OP = "python D:/zaion-rust/eval/harness/sample_mcp003_executor.py"
  $B4OP = "python D:/zaion-rust/eval/harness/sample_be004_executor.py"
  $T3OP = "python D:/zaion-rust/eval/harness/sample_tui003_executor.py"
  $T4OP = "python D:/zaion-rust/eval/harness/sample_tui004_executor.py"
  $S3OP = "python D:/zaion-rust/eval/harness/sample_ses003_executor.py"
  $M3P3OP = "python D:/zaion-rust/eval/harness/sample_mem003_executor.py"
  $M4OP = "python D:/zaion-rust/eval/harness/sample_mem004_executor.py"
  $SES4OP = "python D:/zaion-rust/eval/harness/sample_ses004_executor.py"
  $T5OP = "python D:/zaion-rust/eval/harness/sample_tui005_executor.py"
  $B5OP = "python D:/zaion-rust/eval/harness/sample_be005_executor.py"
  $M5OP = "python D:/zaion-rust/eval/harness/sample_mem005_executor.py"
  $M8OP = "python D:/zaion-rust/eval/harness/sample_mem008_executor.py"
  $S5OP = "python D:/zaion-rust/eval/harness/sample_ses005_executor.py"
  $T6OP = "python D:/zaion-rust/eval/harness/sample_tui006_executor.py"
  $M6OP = "python D:/zaion-rust/eval/harness/sample_mem006_executor.py"
  $M9OP = "python D:/zaion-rust/eval/harness/sample_mem009_executor.py"
  $S6OP = "python D:/zaion-rust/eval/harness/sample_ses006_executor.py"
  $R4OP = "python D:/zaion-rust/eval/harness/sample_rel004_executor.py"
$outRoot = "$env:TEMP\zaion-suite"
Remove-Item -Recurse -Force $outRoot -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $outRoot | Out-Null
$runs = @(
  @("ZAION-300-HERO-001", $HEROOP),
  @("ZAION-300-HERO-007", $SREOP),
  @("ZAION-300-CH-001", $CHANNEL),
  @("ZAION-300-REC-001", $RECOP),
  @("ZAION-300-SEC-006", $SECOP),
  @("ZAION-300-TOOLS-001", $FILEOP),
  @("ZAION-300-MEM-001", $MEMOP),
  @("ZAION-300-SES-001", $SESOP),
  @("ZAION-300-IDP-001", $IDPOP),
  @("ZAION-300-APR-001", $APROP),
  @("ZAION-300-EVD-001", $EVDOP),
  @("ZAION-300-SK-001", $SKOP),
  @("ZAION-300-CTX-002", $CTXOP),
  @("ZAION-300-ONB-002", $ONBOP),
  @("ZAION-300-BE-006", $BEOP),
  @("ZAION-300-GW-005", $GWOP),
  @("ZAION-300-ENV-004", $ENVOP),
  @("ZAION-300-MCP-004", $MCPOP),
  @("ZAION-300-ACP-001", $ACPOP),
  @("ZAION-300-TUI-001", $UIOP),
  @("ZAION-300-REL-002", $R2OP),
  @("ZAION-300-ACP-002", $A2OP),
  @("ZAION-300-BE-002", $B2OP),
  @("ZAION-300-MEM-002", $M2OP),
  @("ZAION-300-SES-002", $S2OP),
  @("ZAION-300-CTX-001", $C1OP),
  @("ZAION-300-SEC-001", $S1OP),
  @("ZAION-300-SK-002", $S2KOP),
  @("ZAION-300-GW-002", $G2OP),
  @("ZAION-300-ENV-003", $E3OP),
  @("ZAION-300-MCP-002", $M2P2OP),
  @("ZAION-300-SEC-004", $SEC4OP),
  @("ZAION-300-HERO-003", $HEROOP),
  @("ZAION-300-HERO-008", $SREOP),
  @("ZAION-300-BE-003", $B3OP),
  @("ZAION-300-HERO-004", $RBOP),
  @("ZAION-300-HERO-010", $RBOP),
  @("ZAION-300-ENV-005", $E5OP),
  @("ZAION-300-ENV-006", $E6OP),
  @("ZAION-300-HERO-005", $H5OP),
  @("ZAION-300-HERO-006", $H6OP),
  @("ZAION-300-HERO-011", $H11OP),
  @("ZAION-300-TUI-002", $T2OP),
  @("ZAION-300-HERO-002", $H2OP),
  @("ZAION-300-CTX-003", $C3OP),
  @("ZAION-300-MCP-003", $M3OP),
  @("ZAION-300-BE-004", $B4OP),
  @("ZAION-300-TUI-003", $T3OP),
  @("ZAION-300-TUI-004", $T4OP),
  @("ZAION-300-SES-003", $S3OP),
  @("ZAION-300-MEM-003", $M3P3OP),
  @("ZAION-300-MEM-004", $M4OP),
  @("ZAION-300-SES-004", $SES4OP),
  @("ZAION-300-TUI-005", $T5OP),
  @("ZAION-300-BE-005", $B5OP),
  @("ZAION-300-MEM-005", $M5OP),
  @("ZAION-300-MEM-008", $M8OP),
  @("ZAION-300-SES-005", $S5OP),
  @("ZAION-300-TUI-006", $T6OP),
  @("ZAION-300-MEM-006", $M6OP),
  @("ZAION-300-MEM-009", $M9OP),
  @("ZAION-300-SES-006", $S6OP),
  @("ZAION-300-REL-004", $R4OP)
)
foreach ($r in $runs) {
  $tid = $r[0]; $exec = $r[1]
  $envDir = "$outRoot\$tid"
  Write-Output "===== $tid ====="
  python $H --run $tid --executor $exec --env $envDir 2>&1 | Select-String -Pattern "success|notes" | Out-String
  $v = python D:/zaion-rust/eval/harness/verifier.py --check $tid --env $envDir 2>&1 | Select-Object -Last 1
  Write-Output "VERIFIER: $v"
}