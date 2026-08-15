$tmp = Join-Path $env:TEMP "zaion-sign-test"
New-Item -ItemType Directory -Force -Path $tmp | Out-Null
python scripts/sign-artifact.py gen-key --key "$tmp\rel.pem" --pub "$tmp\rel.pub.pem"
Write-Output "gen exit: $LASTEXITCODE"
[IO.File]::WriteAllText("$tmp\artifact.bin", "release artifact content v0.1.0")
python scripts/sign-artifact.py sign --key "$tmp\rel.pem" --in "$tmp\artifact.bin" --out "$tmp\artifact.bin.sig"
Write-Output "sign exit: $LASTEXITCODE"
python scripts/sign-artifact.py verify --pub "$tmp\rel.pub.pem" --in "$tmp\artifact.bin" --sig "$tmp\artifact.bin.sig"
Write-Output "verify exit: $LASTEXITCODE (0 = valid)"
[IO.File]::WriteAllText("$tmp\tampered.bin", "release artifact content v0.1.0 TAMPERED")
python scripts/sign-artifact.py verify --pub "$tmp\rel.pub.pem" --in "$tmp\tampered.bin" --sig "$tmp\artifact.bin.sig"
Write-Output "tampered verify exit: $LASTEXITCODE (1 = rejected)"
Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue