# rollback_env_v1

Rollback benchmark environment: a deployed change (broken service.py with
hardcoded port 8080 / threshold 10) must be rolled back to the known-good
version (service.py.known_good reads config.json port 9090 / max_items 5).

Files:
- service.py          broken (deployed) version
- service.py.known_good  correct version to roll back to
- config.json         port 9090, max_items 5

Verification (verifier_rollback): rollback_record.json with known_good=true
and service_healthy=true, after service.py matches known_good.