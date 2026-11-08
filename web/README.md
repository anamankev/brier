# Brier web app

Next.js (App Router), TypeScript strict. It talks only to the public gateway API: REST through
`/api/v1/*` on its own server (which keeps the session token in an httpOnly cookie) and public
market data straight from the gateway's WebSocket.

```bash
npm ci
BRIER_WS_URL=ws://127.0.0.1:8080/v1/ws npx next build
BRIER_GATEWAY_URL=http://127.0.0.1:8080 BRIER_WS_URL=ws://127.0.0.1:8080/v1/ws npx next start --port 3005
```

`BRIER_WS_URL` is needed at build time too: the Content Security Policy that allows the browser
to open that socket is fixed when the app is built.

**A local exchange with activity** (to look at the app): start PostgreSQL (`make up`), then
`BRIER_DEMO_DATABASE_URL=$(make -s db-url) uv run --project services python scripts/demo_stack.py`
starts a test exchange on port 18080 and simulates two hours of trading in fast-forward.

**Tests:** `BRIER_TEST_DATABASE_URL=$(make -s db-url) npx playwright test` runs the G6 flows in
Chromium against a real gateway, with axe-core WCAG 2.2 AA checks on every page.
