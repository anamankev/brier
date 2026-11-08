import { execFileSync, spawn } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { rmSync } from "node:fs";
import { GATEWAY_PORT, PASSWORD, STATE_FILE, call, currentStep, freshTotp, instant, recordStep, type StackState } from "./stack";

const ROOT = resolve(__dirname, "../..");

export default async function setup(): Promise<void> {
  for (const f of [".totp-steps.json", ".newcomer-session.json"]) rmSync(join(__dirname, f), { force: true });
  const db = process.env.BRIER_TEST_DATABASE_URL;
  if (!db) throw new Error("set BRIER_TEST_DATABASE_URL (make db-url) for the browser tests");
  const target = join(ROOT, "target", "test-clock");
  execFileSync("cargo", ["build", "--release", "--locked", "-p", "brier-gateway", "--features", "test-clock", "--target-dir", target], {
    cwd: ROOT,
    stdio: "inherit",
  });
  const binary = join(target, "release", "brier-gateway");
  const work = mkdtempSync(join(tmpdir(), "brier-e2e-"));
  const clockFile = join(work, "clock");
  writeFileSync(clockFile, String(instant("2026-11-10T00:00:00Z")));
  const env = {
    ...process.env,
    BRIER_DATABASE_URL: db,
    BRIER_DATABASE_SCHEMA: `playwright_${Date.now()}`,
    BRIER_DATA_DIR: join(work, "journal"),
    BRIER_LISTEN: `127.0.0.1:${GATEWAY_PORT}`,
    BRIER_TEST_CLOCK_FILE: clockFile,
    BRIER_TOTP_KEY: "42".repeat(32),
    BRIER_DISPUTE_WINDOW_SECONDS: "3600",
    BRIER_TEST_AUTH_PER_MINUTE: "10000",
  };
  const gateway = spawn(binary, ["serve"], { env, stdio: "ignore", detached: true });
  gateway.unref();
  for (let i = 0; ; i++) {
    try {
      await call("GET", "/v1/health");
      break;
    } catch {
      if (i > 150) throw new Error("the gateway did not start");
      await new Promise((r) => setTimeout(r, 200));
    }
  }

  const users: StackState["users"] = {};
  const register = async (name: string, role?: string) => {
    await call("POST", "/v1/register", undefined, { username: name, password: PASSWORD });
    if (role) execFileSync(binary, ["role", name, role, "browser test role"], { env, stdio: "ignore" });
    const { token } = await call<{ token: string }>("POST", "/v1/login", undefined, { username: name, password: PASSWORD });
    users[name] = { token };
  };
  for (const [name, role] of [["ada", "admin"], ["cyd", "admin"], ["tom", undefined], ["lis", "lister"]] as const) await register(name, role);
  for (const name of ["ada", "cyd"]) {
    const { secret } = await call<{ secret: string }>("POST", "/v1/totp/setup", users[name]!.token);
    recordStep(name, currentStep() - 2);
    await call("POST", "/v1/totp/enable", users[name]!.token, { code: await freshTotp(name, secret) });
    const { token } = await call<{ token: string }>("POST", "/v1/login", undefined, { username: name, password: PASSWORD, totp: await freshTotp(name, secret) });
    users[name] = { token, secret };
  }

  // The launch catalogue, converted from TOML by the project's own rulebook loader.
  const catalogue = JSON.parse(
    execFileSync(
      "uv",
      [
        "run", "--project", "services", "--locked", "python", "-c",
        "import json, pathlib; from brier_services.rules import load_toml; " +
          "print(json.dumps([load_toml(p).raw for p in sorted(pathlib.Path('markets').glob('*.toml'))]))",
      ],
      { cwd: ROOT, encoding: "utf8" },
    ),
  ) as { market: number; title: string; schedule: { release: string; close: string } }[];
  const markets: StackState["markets"] = [];
  for (const rulebook of catalogue) {
    const listed = await call<{ rules_hash: string }>("POST", "/v1/admin/markets", users.ada!.token, { rulebook, reason: "browser tests" });
    markets.push({ market: rulebook.market, title: rulebook.title, rules_hash: listed.rules_hash, release: rulebook.schedule.release, close: rulebook.schedule.close });
  }
  // One day of a recurring series, from the lister's own template (forecast high 60 °F), listed
  // as the lister service lists it.
  const event = JSON.parse(
    execFileSync(
      "uv",
      [
        "run", "--project", "services", "--locked", "python", "-c",
        "import json, datetime; from brier_services.catalog import templates as t; " +
          "print(json.dumps(t.temperature(t.CITIES[0], datetime.date(2026, 11, 12), 60)))",
      ],
      { cwd: ROOT, encoding: "utf8" },
    ),
  ) as { market: number; event: { slug: string; title: string; outcome: string } }[];
  for (const rulebook of event) {
    await call("POST", "/v1/admin/markets", users.lis!.token, { rulebook, reason: "browser tests: series listing" });
  }
  const state: StackState = {
    pid: gateway.pid!,
    clockFile,
    users,
    markets,
    event: { slug: event[0]!.event.slug, title: event[0]!.event.title, markets: event.map((r) => ({ market: r.market, outcome: r.event.outcome })) },
  };
  writeFileSync(STATE_FILE, JSON.stringify(state, null, 1));
}
