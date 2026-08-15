$ErrorActionPreference = 'Stop'
$path = 'D:\zaion-rust\eval\benchmarks\zaion_300_v1.json'
$j = Get-Content $path -Raw | ConvertFrom-Json
$exists = $false
foreach ($t in $j.tasks) { if ($t.id -eq 'ZAION-300-REC-001') { $exists = $true } }
if (-not $exists) {
  $t = [pscustomobject]@{
    id = 'ZAION-300-REC-001'; category = 'reliability_security'; slots = 1
    title = 'Recover from a crash at a commit point using the pending journal'
    status = 'planned'
    source = @{ kind = 'zaion_spec'; path = 'plans/zaion-10-10-leap-plan.md'; ref = 'main' }
    acceptance = @{ parity = @('Pending journal items are applied and journal marked committed.'); surpass = @('No duplicate side effects.'); ten_out_of_ten = @('Crash recovery leaves zero data loss.') }
    score = $null; evidence = @(); result = $null; task_type = 'recovery'
  }
  $j.tasks += $t
  $json = $j | ConvertTo-Json -Depth 20
  [System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
  Write-Output 'REC-001 added'
} else { Write-Output 'REC-001 exists' }
