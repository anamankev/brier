// Runs once when the server starts. In production a missing or weak setting stops the server
// instead of letting it run half-configured (an unset proxy key, say, silently loses every
// client address and with it the per-address limits).
export function register() {
  if (process.env.NODE_ENV !== "production" || process.env.BRIER_SKIP_ENV_CHECK === "1") return;
  const problems = envProblems(process.env);
  if (problems.length) throw new Error(`refusing to start: ${problems.join("; ")}`);
}

export function envProblems(env: Record<string, string | undefined>): string[] {
  const out: string[] = [];
  const url = (name: string, schemes: string[]) => {
    const v = env[name];
    if (!v) return out.push(`${name} is not set`);
    try {
      if (!schemes.includes(new URL(v).protocol)) out.push(`${name} must use ${schemes.join(" or ")}`);
    } catch {
      out.push(`${name} is not a URL`);
    }
  };
  url("BRIER_GATEWAY_URL", ["http:", "https:"]);
  url("BRIER_WS_URL", ["wss:"]);
  url("BRIER_SITE_URL", ["https:"]);
  if ((env.BRIER_PROXY_KEY ?? "").length < 32) out.push("BRIER_PROXY_KEY must be at least 32 characters");
  return out;
}
