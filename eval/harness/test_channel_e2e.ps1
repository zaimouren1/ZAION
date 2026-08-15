$H = "D:/zaion-rust/eval/harness/runner.py"
$envDir = "$env:TEMP\zaion-ch-e2e\ZAION-300-CH-001"
python $H --run ZAION-300-CH-001 --executor "python D:/zaion-rust/eval/harness/sample_channel_executor.py" --env $envDir 2>&1 | Out-String
python $H --score "$envDir\result.json" 2>&1 | Out-String
python D:/zaion-rust/eval/harness/verifier.py --check ZAION-300-CH-001 --env $envDir 2>&1 | Out-String