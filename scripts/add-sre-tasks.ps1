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
  @('ZAION-300-HERO-007','hero_mission','Diagnose the SRE incident from the log and fix the service so it honors its config','happy_path','Service binds the configured port and applies the configured threshold.','Fix is minimal and the log patterns are addressed.','SRE fix mission completes under 10 minutes.'),
  @('ZAION-300-HERO-008','hero_mission','Apply a config change, verify health, then roll back on regression','recovery','Config change verified; rollback restores prior behavior.','Rollback has evidence.','Config rollback suite passes with evidence.'),
  @('ZAION-300-ENV-003','environments','Restart the service after a config change without losing state','recovery','Restart picks up config; state preserved.','Restart is clean.','Restart suite passes.')
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
