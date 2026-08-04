# Resource checksums

Checksums are generated only for approved resources declared in `resources/manifests/`. `ApprovedTool` verifies the lowercase SHA-256 at configuration time and immediately before every spawn. A mismatch prevents execution; there is no silent fallback to an unverified file.
