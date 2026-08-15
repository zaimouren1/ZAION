# security_env_v1 Scenario (designer-only)

Two receipts exist under receipts/:
- receipt-valid.json: sha256 matches its payload (VALID)
- receipt-tampered.json: sha256 does NOT match (payload tampered, hash not updated)

Task (agent): verify both receipts with verify_receipt.py and write
verification_report.json: {"results": [{"id": "r1", "valid": true}, {"id": "r2", "valid": false}]}

Mapped tasks: ZAION-300-SEC-006 (tampered receipts rejected)
