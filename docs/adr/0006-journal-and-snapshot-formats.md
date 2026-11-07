# ADR 0006: Journal and snapshot file formats

Date: 2026-11-06  Status: Accepted

## Context
The journal is the source of truth (SPEC §12): every state the exchange ever had must be rebuildable from it, years later, with any later version of the code. Its bytes must therefore be stable and unambiguous, damage must be detectable, and a write interrupted by a crash (a torn tail) must be distinguishable from corruption.

## Options considered
1. **JSON lines** (the replay tool's format). Readable and already specified (`docs/protocol.md`), but large. Its exact bytes depend on the serializer, and a torn line can still be valid JSON.
2. **A general serialization library (bincode, protobuf).** Compact, but the encoding is tied to library versions and schema rules this project does not control.
3. **A hand-written, length-prefixed, checksummed binary format with explicit versioning.**

## Decision
Option 3.
- **Segment file** `journal-<first seq, 20 digits>.log`. Header: 8-byte magic `BRIERJNL`, format version `u16`, first sequence `u64`. Segments rotate at 64 MiB.
- **Record:** `len: u32` (payload bytes), `crc32c: u32` (over `seq` and `payload`), `seq: u64`, then `payload`, all little-endian. CRC-32C (Castagnoli) is checked against the standard check value: "123456789" gives `0xE3069283`.
- **Payload:** one tag byte per command kind, then that command's fields at fixed widths. Optional fields carry a presence byte. Unknown tags or trailing bytes are errors.
- **Snapshot file** `snapshot-<seq, 20 digits>.bin`. Header: magic `BRIERSNP`, version `u16`, `seq: u64`, state hash (32 bytes), body length `u64`, body `crc32c: u32`. The body is a canonical encoding of the engine state (markets, books in time priority, accounts, holdings, escrow, issuance).
- **Reading rules:** at the end of the last segment, a short record or a CRC mismatch is a torn tail, truncated and logged. Anywhere else it is corruption, and recovery stops with an error. Records must have strictly consecutive sequence numbers across segments.

## Consequences
- Old journals stay readable: a format change bumps the version, and readers keep decoding every version they have ever written.
- Debugging needs a tool: `brier-replay` gains a mode that reads a journal and prints the commands as JSON lines.
- Wrong-choice signal: if the format needs frequent changes, introduce a schema description and code generation rather than ad hoc versions.
