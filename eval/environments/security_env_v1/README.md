# security_env_v1

Benchmark scenario: signature-tamper detection.
- receipts/: one valid receipt, one tampered receipt
- verify_receipt.py: checksum verifier (the tool the agent uses)

The agent must verify both and write verification_report.json.
See TASKS.md for the inventory (designer-only).
