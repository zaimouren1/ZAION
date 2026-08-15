$ErrorActionPreference = 'Stop'
$schemaPath = 'D:\zaion-rust\eval\benchmark_manifest.schema.json'
$s = Get-Content $schemaPath -Raw
# 1. add openclaw to baselines.properties (after hermes baseline entry)
$s = $s -replace '("baselines": {s*"type": "object",s*"additionalProperties": false,s*"required": [s*"hermes"s*],s*"properties": {s*"hermes": {s*"$ref": "#/$defs/baseline"s*}s*})', '$1' -replace '"hermes": {s*"$ref": "#/$defs/baseline"s*}', '"hermes": { "$ref": "#/$defs/baseline" }, "openclaw": { "$ref": "#/$defs/baseline" }'
# 2. add task_type and risk_profile to task properties (after result property block, before closing of task def)
$s = $s -replace '("result": {s*"oneOf": [s*{s*"type": "null"s*},s*{s*"$ref": "#/$defs/result"s*}s*]s*})', '$1' -replace '"result": {s*"oneOf": [s*{s*"type": "null"s*},s*{s*"$ref": "#/$defs/result"s*}s*]s*}(s*})', '"result": { "oneOf": [ { "type": "null" }, { "$ref": "#/$defs/result" } ] }, "task_type": { "enum": ["happy_path", "approval", "recovery", "idempotency", "security", "evidence"] }, "risk_profile": { "type": ["object", "null"] }$1'
[System.IO.File]::WriteAllText($schemaPath, $s, (New-Object System.Text.UTF8Encoding($false)))
Write-Output 'schema updated'
# validate JSON
$null = $s | ConvertFrom-Json
Write-Output 'schema JSON valid'
