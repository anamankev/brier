# Load test (G10 U7)

`scripts/load_test.py` against https://132-145-162-222.sslip.io on 2026-11-11, 30 s per level, a mix of the home page, a category page, an event page, the events API (direct and through the web proxy) and health.

| Build | Users | req/s | p50 ms | p95 ms | p99 ms | Errors |
|---|---|---|---|---|---|---|
| No cache | 50 | ~6 | — | — | — | 0 |
| Market rows cached | 50 | ~13 | — | 8,200 | — | 0 |
| Whole events list cached 3 s | 5 | 11.6 | 282 | 744 | 933 | 0 |
| Whole events list cached 3 s | 20 | 14.1 | 1,304 | 3,132 | 3,480 | 0 |
| Whole events list cached 3 s | 50 | 13.7 | 2,851 | 7,935 | 18,614 | 0 |

With the cache, the gateway's own `/v1/events` stays near 0.4 s at the 95th percentile even at 50 users. The limit is now page rendering on the web server (an Always Free E2.1.Micro, 2 small burstable cores). About 14 pages a second is roughly 50,000 an hour, far above expected launch traffic, and nothing failed under load. If traffic grows, the next steps are caching rendered pages for signed-out visitors and moving the web server to a larger free shape when one is available.
