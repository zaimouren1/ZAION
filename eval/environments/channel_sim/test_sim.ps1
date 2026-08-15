$sim = "D:/zaion-rust/eval/environments/channel_sim/channel_sim.py"
$st = "$env:TEMP\cs-state.json"
Remove-Item $st -ErrorAction SilentlyContinue
$proc = Start-Process python -ArgumentList "$sim --port 8085 --token TESTTOKEN --state $st" -PassThru -WindowStyle Hidden
Start-Sleep -Seconds 2
Invoke-RestMethod -Uri "http://127.0.0.1:8085/sim/reset" -Method Post | Out-Null
$body = @{ update_id = 1; message = @{ text = "hello zaion"; chat = @{ id = 42 } } } | ConvertTo-Json -Depth 5
Invoke-RestMethod -Uri "http://127.0.0.1:8085/sim/state" -Method Post -Body $body -ContentType "application/json" | Out-Null
$upd = Invoke-RestMethod -Uri "http://127.0.0.1:8085/botTESTTOKEN/getUpdates" -Method Post
Write-Output "update text: $($upd.result[0].message.text)"
$reply = @{ chat_id = 42; text = "reply from zaion" } | ConvertTo-Json
$r = Invoke-RestMethod -Uri "http://127.0.0.1:8085/botTESTTOKEN/sendMessage" -Method Post -Body $reply -ContentType "application/json"
Write-Output "reply id: $($r.result.message_id)"
$state = Invoke-RestMethod -Uri "http://127.0.0.1:8085/sim/state"
Write-Output "sent count: $($state.sent.Count) | updates left: $($state.updates.Count)"
Write-Output "sent text: $($state.sent[0].text)"
Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
Remove-Item $st -ErrorAction SilentlyContinue