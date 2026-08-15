$ErrorActionPreference = 'Stop'
$path = 'D:\zaion-rust\eval\benchmarks\zaion_300_v1.json'
$j = Get-Content $path -Raw | ConvertFrom-Json
foreach ($t in $j.tasks) {
  if ($t.id -eq 'ZAION-300-HERO-006') {
    $t.output.format = 'JSON object with fields: root_cause (string), documented (bool true), evidence_linked (bool true), alert (string)'
  }
}
$json = $j | ConvertTo-Json -Depth 20
[System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
Write-Output 'HERO-006 output format detailed'
