$ErrorActionPreference = 'Stop'
$path = 'D:\zaion-rust\eval\benchmarks\zaion_300_v1.json'
$j = Get-Content $path -Raw | ConvertFrom-Json
$exists = $false
foreach ($t in $j.tasks) { if ($t.id -eq 'ZAION-300-SEC-006') { $exists = $true } }
if (-not $exists) {
  $t = [pscustomobject]@{
    id = 'ZAION-300-SEC-006'; category = 'reliability_security'; slots = 1
    title = 'Verify receipts and flag the tampered one in a report'
    status = 'planned'
    source = @{ kind = 'zaion_spec'; path = 'plans/zaion-10-10-leap-plan.md'; ref = 'main' }
    acceptance = @{ parity = @('Verification report correctly identifies valid and tampered receipts.'); surpass = @('Report is machine-checkable.'); ten_out_of_ten = @('All tamper scenarios are detected.') }
    score = $null; evidence = @(); result = $null; task_type = 'security'
  }
  $j.tasks += $t
  $json = $j | ConvertTo-Json -Depth 20
  [System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
  Write-Output 'SEC-006 added'
} else { Write-Output 'SEC-006 exists' }
