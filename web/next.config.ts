import type { NextConfig } from "next";

// The gateway's WebSocket, which the browser connects to directly for public market data. The
// security headers are fixed at build time, so set BRIER_WS_URL when building as well as when
// serving (web/README.md).
const ws = process.env.BRIER_WS_URL ?? "ws://127.0.0.1:8080/v1/ws";

const config: NextConfig = {
  poweredByHeader: false,
  reactStrictMode: true,
  async headers() {
    return [
      {
        source: "/:path*",
        headers: [
          {
            key: "Content-Security-Policy",
            // Next.js inlines its bootstrap scripts, so scripts need 'unsafe-inline' until the
            // app uses per-request nonces (docs/threat-model.md, G6).
            value: [
              "default-src 'self'",
              "script-src 'self' 'unsafe-inline'",
              "style-src 'self' 'unsafe-inline'",
              "img-src 'self' data:",
              `connect-src 'self' ${ws}`,
              "frame-ancestors 'none'",
              "base-uri 'self'",
              "form-action 'self'",
            ].join("; "),
          },
          { key: "X-Content-Type-Options", value: "nosniff" },
          { key: "Referrer-Policy", value: "same-origin" },
          { key: "X-Frame-Options", value: "DENY" },
        ],
      },
    ];
  },
};

export default config;
