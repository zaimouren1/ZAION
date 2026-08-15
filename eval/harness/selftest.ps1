$H = "D:/zaion-rust/eval/harness/runner.py"
Write-Output "===== list (first 5) ====="
python $H --list 2>&1 | Select-Object -First 8 | Out-String
Write-Output "===== setup + dry-run + score ====="
python $H --run ZAION-300-HERO-001 --dry-run 2>&1 | Out-String
python $H --score "$env:TEMP\zaion-eval\ZAION-300-HERO-001\result.json" 2>&1 | Out-String
Write-Output "===== report ====="
python $H --report "$env:TEMP\zaion-eval" 2>&1 | Out-String