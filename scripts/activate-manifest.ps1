$p = 'D:\zaion-rust\eval\benchmarks\zaion_300_v1.json'
$j = Get-Content $p -Raw | ConvertFrom-Json
$j.status = 'active'
$json = $j | ConvertTo-Json -Depth 20
[System.IO.File]::WriteAllText($p, $json, (New-Object System.Text.UTF8Encoding($false)))
Write-Output 'manifest status -> active'
