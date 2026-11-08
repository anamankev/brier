// Shared by the setup and the tests: ports, the test exchange's clock, TOTP codes, and direct
// gateway calls for the parts of a flow that are not under test.
import { createHmac } from "node:crypto";
import { readFileSync, writeFileSync, renameSync } from "node:fs";
import { join } from "node:path";

export const GATEWAY_PORT = 18080;
export const WEB_PORT = 3100;
export const GATEWAY = `http://127.0.0.1:${GATEWAY_PORT}`;
export const PASSWORD = "a long enough password";
export const STATE_FILE = join(__dirname, ".state.json");

export type StackState = {
  pid: number;
  clockFile: string;
  users: Record<string, { token: string; secret?: string }>;
  markets: { market: number; title: string; rules_hash: string; release: string; close: string }[];
};

export function state(): StackState {
  return JSON.parse(readFileSync(STATE_FILE, "utf8")) as StackState;
}

export function instant(iso: string): number {
  return Math.floor(Date.parse(iso) / 1000);
}

export function iso(t: number): string {
  return new Date(t * 1000).toISOString().replace(".000Z", "Z");
}

export function setClock(t: number): void {
  const file = state().clockFile;
  writeFileSync(`${file}.tmp`, String(t));
  renameSync(`${file}.tmp`, file);
}

export function now(): number {
  return Number(readFileSync(state().clockFile, "utf8"));
}

function base32Decode(s: string): Buffer {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let bits = 0;
  let value = 0;
  const out: number[] = [];
  for (const c of s) {
    value = (value << 5) | alphabet.indexOf(c);
    bits += 5;
    if (bits >= 8) {
      out.push((value >>> (bits - 8)) & 0xff);
      bits -= 8;
    }
  }
  return Buffer.from(out);
}

/** The RFC 6238 code (SHA-1, 6 digits, 30 s) for a base32 secret, `offset` steps from now
 * (by the real clock, which the gateway uses for TOTP). */
export function totp(secret: string, offset = 0): string {
  const step = Math.floor(Date.now() / 1000 / 30) + offset;
  const counter = Buffer.alloc(8);
  counter.writeBigUInt64BE(BigInt(step));
  const digest = createHmac("sha1", base32Decode(secret)).update(counter).digest();
  const o = digest[digest.length - 1]! & 0x0f;
  const word = digest.readUInt32BE(o) & 0x7fffffff;
  return String(word % 1_000_000).padStart(6, "0");
}

export async function call<T = unknown>(method: string, path: string, token?: string, body?: unknown): Promise<T> {
  const headers: Record<string, string> = { "content-type": "application/json" };
  if (token) headers.authorization = `Bearer ${token}`;
  const res = await fetch(`${GATEWAY}${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  const text = await res.text();
  if (!res.ok) throw new Error(`${method} ${path}: ${res.status} ${text}`);
  return (text ? JSON.parse(text) : null) as T;
}

export async function until(what: string, check: () => Promise<boolean>, ms = 15_000): Promise<void> {
  const end = Date.now() + ms;
  while (!(await check())) {
    if (Date.now() > end) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 200));
  }
}
