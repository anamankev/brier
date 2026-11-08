import type { Metadata } from "next";
import Link from "next/link";
import { Barlow_Condensed, Inter } from "next/font/google";
import { gateway, me } from "@/lib/server";
import { credits } from "@/lib/format";
import type { MarketRow, Portfolio } from "@/lib/types";
import { Mark, SearchIcon } from "@/components/Icons";
import { SignOut } from "@/components/SignOut";
import "./globals.css";

const display = Barlow_Condensed({ subsets: ["latin"], weight: ["600", "700"], variable: "--font-display" });
const sans = Inter({ subsets: ["latin"], variable: "--font-sans" });

export const metadata: Metadata = {
  title: { default: "Brier · Forecast the economy", template: "%s · Brier" },
  description: "Trade yes/no contracts on economic data, rates, and weather. Every market's rules are pinned and every outcome comes with its evidence.",
};

const CATEGORIES = [
  ["economics", "Economics"],
  ["rates", "Rates"],
  ["weather", "Weather"],
] as const;

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  const [user, list] = await Promise.all([me(), gateway<{ markets: MarketRow[] }>("/v1/markets")]);
  const portfolio = user ? await gateway<Portfolio>("/v1/portfolio") : null;
  const markets = list.ok ? list.body.markets.filter((m) => m.title) : [];
  const count = (c: string) => markets.filter((m) => m.category === c).length;
  return (
    <html lang="en" className={`${display.variable} ${sans.variable}`}>
      <body>
        <a className="skip" href="#main">
          Skip to content
        </a>
        <header className="site-header">
          <div className="page bar">
            <Link href="/" className="logo" aria-label="Brier home">
              <Mark />
              Brier
            </Link>
            <nav className="nav" aria-label="Main">
              <Link href="/">Markets</Link>
              <Link href="/leaderboards">Leaderboards</Link>
              {user && <Link href="/portfolio">Portfolio</Link>}
              {user?.role === "admin" && <Link href="/admin">Admin</Link>}
            </nav>
            <form className="search" action="/" role="search">
              <SearchIcon />
              <label className="sr-only" htmlFor="q">
                Search markets
              </label>
              <input id="q" name="q" type="search" placeholder="Search markets" autoComplete="off" />
            </form>
            <div className="who">
              {user ? (
                <>
                  {portfolio?.ok && (
                    <Link href="/portfolio" className="balance">
                      Cash
                      <strong>{credits(portfolio.body.cash)}</strong>
                    </Link>
                  )}
                  <Link href="/account" className="btn small">
                    {user.username}
                  </Link>
                  <SignOut />
                </>
              ) : (
                <>
                  <Link href="/signin" className="btn">
                    Log in
                  </Link>
                  <Link href="/register" className="btn primary">
                    Sign up
                  </Link>
                </>
              )}
            </div>
          </div>
          <nav className="page cats" aria-label="Categories">
            <Link href="/">All markets</Link>
            {CATEGORIES.map(([id, name]) => (
              <Link key={id} href={`/?category=${id}`}>
                {name}
                <span className="count">{count(id)}</span>
              </Link>
            ))}
            {user && <Link href="/scorecard">Your scorecard</Link>}
          </nav>
        </header>
        <main id="main" className="page">
          {children}
        </main>
        <footer className="site-footer">
          <div className="page">
            <div className="fcols">
              <div>
                <Link href="/" className="logo" aria-label="Brier home">
                  <Mark size={24} />
                  Brier
                </Link>
                <p className="muted" style={{ fontSize: 14, maxWidth: 280 }}>
                  A prediction market for economic data, rates, and weather, scored by the Brier score.
                </p>
              </div>
              <div>
                <h2>Trade</h2>
                <ul>
                  <li><Link href="/">Markets</Link></li>
                  <li><Link href="/leaderboards">Leaderboards</Link></li>
                  <li><Link href="/scorecard">Your scorecard</Link></li>
                </ul>
              </div>
              <div>
                <h2>How it settles</h2>
                <ul>
                  <li><a href="https://github.com/anamankev/brier/blob/main/docs/rulebook.md">Rulebooks</a></li>
                  <li><a href="https://github.com/anamankev/brier/blob/main/docs/resolution.md">Resolution and disputes</a></li>
                  <li><a href="https://github.com/anamankev/brier/blob/main/docs/data-licenses.md">Data sources</a></li>
                </ul>
              </div>
              <div>
                <h2>Developers</h2>
                <ul>
                  <li><a href="https://github.com/anamankev/brier/blob/main/docs/api/openapi.yaml">REST API</a></li>
                  <li><a href="https://github.com/anamankev/brier/blob/main/docs/api/asyncapi.yaml">WebSocket API</a></li>
                  <li><a href="https://github.com/anamankev/brier">Source code</a></li>
                </ul>
              </div>
            </div>
            <div className="legal">
              <p style={{ margin: 0 }}>
                Brier uses play money. Credits are granted once at sign-up, have no cash value, and cannot be bought,
                sold, withdrawn, or transferred.
              </p>
              <p style={{ margin: 0 }}>
                Data: U.S. Bureau of Labor Statistics; Federal Reserve Bank of St. Louis (FRED®); NOAA National Centers
                for Environmental Information. This product uses the FRED® API but is not endorsed or certified by the
                Federal Reserve Bank of St. Louis. By using FRED® data here you are bound by the{" "}
                <a href="https://fred.stlouisfed.org/docs/api/terms_of_use.html">FRED® API Terms of Use</a>.
              </p>
            </div>
          </div>
        </footer>
      </body>
    </html>
  );
}
