$ErrorActionPreference = 'Stop'
$path = 'D:\zaion-rust\eval\benchmarks\zaion_300_v1.json'
$j = Get-Content $path -Raw | ConvertFrom-Json
foreach ($t in $j.tasks) {
  if ($t.id -eq 'ZAION-300-HERO-006') {
    $t | Add-Member -NotePropertyName output -NotePropertyValue @{ path = 'hero006_record.json'; format = 'JSON with root_cause (string) and evidence_linked (bool)' } -Force
  }
  if ($t.id -eq 'ZAION-300-BE-002') {
    $t | Add-Member -NotePropertyName output -NotePropertyValue @{ path = 'be002_record.json'; format = 'JSON with run_1/run_2 scores and identical (bool)' } -Force
  }
}
$json = $j | ConvertTo-Json -Depth 20
[System.IO.File]::WriteAllText($path, $json, (New-Object System.Text.UTF8Encoding($false)))
Write-Output 'output fields added to HERO-006 + BE-002'
