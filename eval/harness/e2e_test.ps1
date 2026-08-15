$H = "D:/zaion-rust/eval/harness/runner.py"
$envDir = "$env:TEMP\zaion-eval-e2e\ZAION-300-HERO-001"
python $H --run ZAION-300-HERO-001 --executor "python D:/zaion-rust/eval/harness/sample_executor.py" --env $envDir 2>&1 | Out-String
python $H --score "$envDir\result.json" 2>&1 | Out-String