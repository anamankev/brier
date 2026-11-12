// Team colours for game badges. Colours and abbreviations are facts about a team, not its logo,
// so Brier draws its own badge from them instead of using league or club artwork.

type Colors = { bg: string; fg: string };

const NBA: Record<string, string> = {
  atl: "#E03A3E", bos: "#007A33", bkn: "#000000", cha: "#1D1160", chi: "#CE1141", cle: "#860038",
  dal: "#00538C", den: "#0E2240", det: "#C8102E", gsw: "#1D428A", hou: "#CE1141", ind: "#002D62",
  lac: "#C8102E", lal: "#552583", mem: "#5D76A9", mia: "#98002E", mil: "#00471B", min: "#0C2340",
  nop: "#0C2340", nyk: "#006BB6", okc: "#007AC1", orl: "#0077C0", phi: "#006BB6", phx: "#1D1160",
  por: "#E03A3E", sac: "#5A2D81", sas: "#C4CED4", tor: "#CE1141", uta: "#002B5C", was: "#002B5C",
};

const NFL: Record<string, string> = {
  ari: "#97233F", atl: "#A71930", bal: "#241773", buf: "#00338D", car: "#0085CA", chi: "#0B162A",
  cin: "#FB4F14", cle: "#311D00", dal: "#003594", den: "#FB4F14", det: "#0076B6", gb: "#203731",
  hou: "#03202F", ind: "#002C5F", jax: "#006778", kc: "#E31837", lv: "#A5ACAF", lac: "#0080C6",
  lar: "#003594", mia: "#008E97", min: "#4F2683", ne: "#002244", no: "#D3BC8D", nyg: "#0B2265",
  nyj: "#125740", phi: "#004C54", pit: "#FFB612", sf: "#AA0000", sea: "#002244", tb: "#D50A0A",
  ten: "#0C2340", was: "#5A1414",
};

const SOCCER = new Set(["premier-league", "la-liga", "serie-a", "bundesliga", "ligue-1"]);
// Soccer clubs are many and change every season: a calm tone from the code keeps badges distinct.
const NEUTRAL = ["#1F3A5F", "#3B2F5C", "#2F4F3A", "#5C2F35", "#4A4030", "#284B57", "#4B3A2A", "#33405C"];

/** Readable text on a background: dark on light colours, white on dark ones. */
function ink(hex: string): string {
  const n = parseInt(hex.slice(1), 16);
  const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  return 0.299 * r + 0.587 * g + 0.114 * b > 160 ? "#0B0D10" : "#FFFFFF";
}

function colors(league: string, code: string): Colors {
  const table = league === "nba" ? NBA : league === "nfl" ? NFL : null;
  let bg = table?.[code];
  if (!bg) bg = NEUTRAL[[...code].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7) % NEUTRAL.length]!;
  return { bg, fg: ink(bg) };
}

export type Matchup = { league: string; away: { code: string } & Colors; home: { code: string } & Colors };

/** The two teams of a game event, from its slug ("nba-orl-bos-2026-11-17"), or null. */
export function matchup(slug: string | undefined): Matchup | null {
  if (!slug) return null;
  const m = /^(nba|nfl|premier-league|la-liga|serie-a|bundesliga|ligue-1)-([a-z0-9]+)-([a-z0-9]+)-\d{4}-\d{2}-\d{2}$/.exec(slug);
  if (!m) return null;
  const [, league, away, home] = m as unknown as [string, string, string, string];
  return { league, away: { code: away, ...colors(league, away) }, home: { code: home, ...colors(league, home) } };
}

export const isSoccer = (league: string) => SOCCER.has(league);
