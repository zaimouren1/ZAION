$ErrorActionPreference = 'Stop'
$path = 'D:\zaion-rust\eval\benchmarks\zaion_300_v1.json'
$j = Get-Content $path -Raw | ConvertFrom-Json
$exists = $false
foreach ($t in $j.tasks) { if ($t.id -eq 'ZAION-300-MEM-001') { $exists = $true } }
if (-not $exists) {
  $t = [pscustomobject]@{
    id = 'ZAION-300-MEM-001'; category = 'memory'; slots = 1
    title = 'Write a memory atom with source attribution'
    status = 'planned'
    source = @{ kind = 'zaion_spec'; path = 'plans/zaion-10-10-leap-plan.md'; ref = 'main' }
    acceptance = @{ parity = @('A memory atom is written with text and a source binding.'); surpass = @('Source is attributable and verifiable.'); ten_out_of_ten = @('All memory writes are source-attributed.') }
    score = $null; evidence = @(); result = $null; task_type = 'evidence'
  }
  $j.tasks += $t
  $json = $j | ConvertTo-Json -Depth 20
  [System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
  Write-Output 'MEM-001 created'
} else { Write-Output 'MEM-001 exists' }
