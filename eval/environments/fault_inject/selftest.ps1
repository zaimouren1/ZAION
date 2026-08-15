$tmp = Join-Path $env:TEMP "fi-test"
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
$FI = "D:/zaion-rust/eval/environments/fault_inject/fault_inject.py"
Write-Output "===== kill-after ====="
python $FI kill-after python -c "import time,sys; [print(chr(99)+chr(111)+chr(109)+chr(109)+chr(105)+chr(116)+chr(32)+str(i), flush=True) or time.sleep(0.05) for i in range(10)]" --after 3 --match commit 2>&1 | Out-String
Write-Output "kill exit: $LASTEXITCODE"
Write-Output "===== tamper ====="
$sig = Join-Path $tmp "sig.txt"
[IO.File]::WriteAllText($sig, "hello world")
python $FI tamper --file $sig --offset 0 2>&1 | Out-String
Write-Output "tamper exit: $LASTEXITCODE"
Write-Output "===== reorder + repeat ====="
$ev = Join-Path $tmp "events.jsonl"
[IO.File]::WriteAllText($ev, "1`n2`n3`n")
python $FI reorder --file $ev 2>&1 | Out-String
python $FI repeat --file $ev --times 2 2>&1 | Out-String
Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue