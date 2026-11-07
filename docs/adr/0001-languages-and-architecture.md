# ADR 0001: Languages and process architecture

Date: 2026-11-06  Status: Accepted

## Context
The order path (gateway, sequencer, engine, ledger, journal) must be deterministic, crash-safe, and fast enough that measured latency and throughput are credible to exchange engineers (SPEC §11). The rest of the system (resolvers, market maker, surveillance, simulation) is data work where development speed and the existing Python quant code (twofold) matter more than nanoseconds. Constraints: $0 tooling, one developer, a 16 GB fanless laptop.

## Options considered
1. **Python everywhere** (FastAPI, like twofold). One language and the fastest start. A pure-Python engine runs in the tens to low hundreds of thousands of operations per second, garbage collection adds latency jitter, and production matching engines are almost never written in Python. It would not meet SPEC §11.
2. **Java or another JVM language** (the LMAX approach). Proven for exchanges, but the JVM needs GC tuning to avoid pauses, and it adds a third runtime with no existing code to reuse.
3. **C++.** The industry incumbent for low latency. Memory-safety bugs in a financial ledger are a real risk for one developer, and the build tooling is heavier.
4. **Rust for the order path, Python for services and research, TypeScript for the web.** No garbage collector, memory safety enforced by the compiler, strong property-testing and fuzzing tools (proptest, cargo-fuzz), and a single static binary. Cost: a learning curve, and a language boundary (crossed only through the journal and the public API, never through in-process bindings).

## Decision
Option 4. The engine, ledger, risk checks, journal, and gateway are one Rust workspace. Python services talk to the core only through the public API or by reading the event stream, exactly as an external participant would. The web app uses Next.js, as in twofold.

## Consequences
- Easier: credible performance numbers; whole bug classes (data races, use-after-free) ruled out by the compiler; the Python reference engine (SPEC §3.4) is independent of the Rust code by construction.
- Harder: two toolchains in CI; the Rust learning curve is part of every G1-G4 unit (the teaching protocol covers it).
- Required: the Rust toolchain installed via rustup (G0).
- Wrong-choice signal: if G1 shows the Rust engine is not materially faster than a Python one on the same workload, or if development stalls on the language rather than the design, revisit this ADR.
