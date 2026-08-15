# Gateway S1 comparison: serve (raw) vs serve-unified (axum) response consistency.
param([string]$Bin = "target/debug/zaion.exe")
$ErrorActionPreference = "Continue"
if (-not (Test-Path $Bin)) { Write-Error "binary not found: $Bin"; exit 1 }

$env:ZAION_GATEWAY_BIND = "127.0.0.1:17841"
$p1 = Start-Process $Bin -ArgumentList "gateway","serve" -PassThru -WindowStyle Hidden -RedirectStandardError "$env:TEMP\gw-serve-err.log"
$env:ZAION_GATEWAY_BIND = "127.0.0.1:17842"
$p2 = Start-Process $Bin -ArgumentList "gateway","serve-unified" -PassThru -WindowStyle Hidden -RedirectStandardError "$env:TEMP\gw-unified-err.log"
Start-Sleep -Seconds 4

$paths = @("/health", "/", "/api/v1/events/stream", "/mcp/v1/call")
Write-Output ("{0,-26} {1,-10} {2,-10} {3}" -f "PATH", "RAW", "UNIFIED", "MATCH")
foreach ($path in $paths) {
  $raw = curl.exe -s -m 3 -o NUL -w "%{http_code}" "http://127.0.0.1:17841$path" 2>$null
  $uni = curl.exe -s -m 3 -o NUL -w "%{http_code}" "http://127.0.0.1:17842$path" 2>$null
  $match = if ($raw -eq $uni) { "OK" } else { "DIFF" }
  Write-Output ("{0,-26} {1,-10} {2,-10} {3}" -f $path, $raw, $uni, $match)
}

Write-Output "===== /health bodies ====="
$rb = curl.exe -s -m 3 "http://127.0.0.1:17841/health" 2>$null
$ub = curl.exe -s -m 3 "http://127.0.0.1:17842/health" 2>$null
Write-Output ("raw:     {0}" -f $rb)
Write-Output ("unified: {0}" -f $ub)

Stop-Process -Id $p1.Id, $p2.Id -Force -ErrorAction SilentlyContinue
Write-Output "===== comparison done ====="