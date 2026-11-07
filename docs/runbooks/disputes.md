# Runbook: resolutions and disputes

For admins. The workflow and its rules are in `docs/resolution.md`; this page is what to do. Every action below needs a reason, which goes into the audit log with your name.

## A proposal arrives
1. Read it: `GET /v1/markets/{id}/resolution`. The newest entry has the outcome, the evidence record, and its hash.
2. Check the rulebook: `GET /v1/markets/{id}/rules`. `journal.matches` must be `true`. If it is not, stop and treat it as an incident (below).
3. Check the value against the source yourself, on the source's own page (the rulebook's `source.url`), for the rulebook's observation period. The record's `value` is already rounded to the series' precision.
4. Check the decision: `value op threshold` from the rulebook. A void needs its `void_condition`; `no_value_by_deadline` needs `retrieved_at` at or after the deadline.
5. If the proposal is yours, you cannot confirm it. Ask another admin.
6. Right: `POST /v1/admin/resolutions/{id}/confirm`. The dispute window (24 h) starts.
7. Wrong: `POST /v1/admin/resolutions/{id}/withdraw`. The resolver proposes again on its next pass (every 5 minutes); if the source itself is wrong, see "The source is wrong".

To re-check the evidence offline, with the raw response from the resolver's store (`var/evidence/<body_sha256>`):
```python
from brier_services.resolution.evidence import EvidenceStore, recompute
from brier_services.rules import validate
# rule = validate(GET /v1/markets/{id}/rules ["rulebook"]); record = the resolution's "evidence"
recompute(record, rule, EvidenceStore())  # -> (outcome, void_condition)
```

## A dispute arrives
A dispute stops settlement until it is ruled on. You cannot rule on a dispute of your own proposal.
1. Read the dispute's reason (audit log, action `file_dispute`) and repeat steps 1-4 above.
2. Common disputes and their answers:
   - "The value was revised." Rejected: the rulebook says the first release counts.
   - "The value is different on another site." Rejected unless the rulebook's source itself shows a different value for that period.
   - "Wrong period" or "wrong arithmetic." Check the observation period and the transform. If the dispute is right, uphold it.
3. Uphold: `POST /v1/admin/disputes/{id}/rule` with `{"decision": "uphold", "reason": ...}`. The resolution is withdrawn, its other open disputes become moot, and a new proposal starts over.
4. Reject: the same call with `"reject"`. Once every dispute is ruled on and the window has ended, the gateway settles.

## The source is wrong, or gone
- **The series was discontinued or redefined before publishing the value.** Propose void by hand, as an admin: `POST /v1/markets/{id}/resolution` with `"outcome": "void"`, `"void_condition": "source_discontinued"`, and an evidence record showing it (the source's notice, by hash), retrieved after the release time. The resolver never proposes this itself.
- **The source published nothing by the deadline.** The resolver proposes `no_value_by_deadline` on its own. Confirm it like any other proposal.
- **The source published a value that is plainly wrong** (a typo the agency later corrected). The rulebook's revision policy is "first release", so the first value stands unless the rulebook's void conditions apply. Do not settle on a value the rulebook does not allow; if in doubt, leave it unconfirmed and raise it.

## Incidents
- **`journal.matches` is false** for a market's rules, or the resolution view's `journal` shows a settlement whose outcome or evidence hash differs from the settled resolution. Stop confirming anything for that market. The journal is the truth; find out how the stored row changed (database access, the audit log).
- **The settler logs "settled in the journal differently".** It has stopped settling that resolution. Compare `GET /v1/markets/{id}/resolution` (`journal` field) with the resolution row; do not edit rows by hand to make them agree.
- **A resolver key may be compromised.** Revoke it (`DELETE /v1/keys/{id}` with the resolver's session), withdraw its open proposals, and issue a new key. A resolver can only propose, so nothing settles without an admin's confirmation.
