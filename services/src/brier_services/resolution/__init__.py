"""Resolvers: fetch a market's public source after its release time, read the value exactly,
decide the outcome, and keep the evidence (docs/resolution.md, G5 plan).

- ``fetch``: HTTPS to a fixed list of hosts, no redirects, bounded size.
- ``bls``, ``fred``, ``noaa``: the request for a rulebook and a strict parser of the response.
- ``values``: exact arithmetic (``Fraction``) and the decision.
- ``evidence``: the evidence record, the raw-response store, and offline recomputation.
- ``resolver``: one rulebook from fetch to proposal.

A response the parser cannot read is an error, never a guess: no proposal is made.
"""
