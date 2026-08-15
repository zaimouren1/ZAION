$p = 'D:\zaion-rust\eval\benchmarks\zaion_300_v1.json'
$j = Get-Content $p -Raw | ConvertFrom-Json
$j.baselines.hermes.mirror_status = 'refreshed_1f8fdc7bd8'
$j.baselines.hermes.commit = '1f8fdc7bd824c8d07e3cefe109bd96425ec3171f'
$j.baselines.hermes.commit_date = '2026-08-14 11:23:40 UTC'
$json = $j | ConvertTo-Json -Depth 20
[System.IO.File]::WriteAllText($p, $json, (New-Object System.Text.UTF8Encoding($false)))
Write-Output 'baseline refreshed'
