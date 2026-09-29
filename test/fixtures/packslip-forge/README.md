# Forge identity fixture

`hk-v2.3.0.sigstore.json` is the packslip `jdx/hk` published with its v2.3.0
GitHub release, signed keylessly by its release workflow, copied unmodified
from jdx/packslip's `tests/fixtures`. Its Fulcio certificate records
repository ID 922514152 and owner ID 216188, which is what
`gh api repos/jdx/hk --jq '.id,.owner.id'` gives. Unit tests verify it offline
against the embedded trusted root.
