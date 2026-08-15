$p = 'D:\zaion-rust\eval\benchmarks\zaion_300_v1.json'
$j = Get-Content $p -Raw | ConvertFrom-Json
$j.baselines.openclaw.commit = '94cdb6c46e12fc5b02cec705035c00d3492fd3f9'
$j.baselines.openclaw.commit_date = '2026-08-14 17:27:11 +0530'
$j.baselines.openclaw.mirror = 'D:/zaion-reference/openclaw-latest'
$json = $j | ConvertTo-Json -Depth 20
[System.IO.File]::WriteAllText($p, $json, (New-Object System.Text.UTF8Encoding($false)))
Write-Output 'openclaw baseline updated'
