import type { Metadata } from "next";
import Link from "next/link";
import { Barlow_Condensed, Inter } from "next/font/google";
import { gateway, me } from "@/lib/server";
import { credits } from "@/lib/format";
import type { Portfolio } from "@/lib/types";
import { CATEGORIES} from "@/lib/events";
import { listEvents } from "@/lib/events-server";
import { Mark, SearchIcon } from "@/components/Icons";
import { UserMenu } from "@/components/UserMenu";
import { TermsUpdate } from "@/components/TermsUpdate";
import { CookieBanner, CookieSettingsLink } from "@/components/CookieBanner";
import { DailyCredits } from "@/components/DailyCredits";
import { Bell } from "@/components/Bell";
import { SITE_URL } from "@/lib/site";
import "./globals.css";

const display = Barlow_Condensed({ subsets: ["latin"], weight: ["600", "700"], variable: "--font-display" });
const sans = Inter({ subsets: ["latin"], variable: "--font-sans" });

export const metadata: Metadata = {
  metadataBase: new URL(SITE_URL),
  applicationName: "Brier",
  openGraph: { siteName: "Brier", type: "website" },
  twitter: { card: "summary_large_image" },
  title: { default: "Brier · Trade what happens next", template: "%s · Brier" },
  description:
    "A play-money prediction market on things public data settles: city temperatures, earthquakes, geomagnetic storms, inflation, jobs, and rates. Every market's rules are pinned and every outcome comes with its evidence.",
};

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  const [user, open] = await Promise.all([me(), listEvents({ status: "open", limit: 0 })]);
  const [portfolio, allowance] = user
    ? await Promise.all([gateway<Portfolio>("/v1/portfolio"), gateway<{ amount: number; claimed_today: boolean }>("/v1/allowance")])
    : [null, null];
  const count = (c: string) => open?.categories[c] ?? 0;
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
            <form className="search" action="/search" role="search">
              <SearchIcon />
              <label className="sr-only" htmlFor="q">
                Search markets
              </label>
              <input id="q" name="q" type="search" placeholder="Search markets" autoComplete="off" />
            </form>
            <div className="who">
              <Link href="/search" className="search-link" aria-label="Search markets">
                <SearchIcon />
              </Link>
              {user ? (
                <>
                  {allowance?.ok && !allowance.body.claimed_today && <DailyCredits amount={allowance.body.amount} />}
                  {portfolio?.ok && (
                    <Link href="/portfolio" className="balance">
                      Cash
                      <strong>{credits(portfolio.body.cash)}</strong>
                    </Link>
                  )}
                  <Bell />
                  <UserMenu username={user.username} admin={user.role === "admin"} cash={portfolio?.ok ? credits(portfolio.body.cash) : undefined} />
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
          <nav className="page cats" aria-label="Browse">
            <Link href="/">Trending</Link>
            <Link href="/new">New</Link>
            <Link href="/movers">Movers</Link>
            <span className="sep" aria-hidden="true" />
            {CATEGORIES.map((c) => (
              <Link key={c.slug} href={`/c/${c.slug}`}>
                {c.name}
                {count(c.slug) > 0 && <span className="count">{count(c.slug)}</span>}
              </Link>
            ))}
            <span className="sep" aria-hidden="true" />
            <Link href="/watchlist">Watchlist</Link>
            <Link href="/leaderboards" className="cats-phone">Leaderboards</Link>
            {user && <Link href="/scorecard">Your scorecard</Link>}
          </nav>
        </header>
        <main id="main" className="page">
          {user && user.terms_current === false && <TermsUpdate />}
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
                  A prediction market on what public data settles, scored by the Brier score. Play money only.
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
                <h2>How it works</h2>
                <ul>
                  <li><Link href="/rules">House rules</Link></li>
                  <li><a href="https://github.com/anamankev/brier/blob/main/docs/rulebook.md">Rulebooks</a></li>
                  <li><a href="https://github.com/anamankev/brier/blob/main/docs/resolution.md">Settlement and disputes</a></li>
                  <li><a href="https://github.com/anamankev/brier/blob/main/docs/data-licenses.md">Data sources</a></li>
                </ul>
              </div>
              <div>
                <h2>Legal</h2>
                <ul>
                  <li><Link href="/terms">Terms of service</Link></li>
                  <li><Link href="/privacy">Privacy policy</Link></li>
                  <li><Link href="/cookies">Cookie policy</Link></li>
                  <li><CookieSettingsLink /></li>
                  <li><Link href="/credits">Photo credits</Link></li>
                  <li><a href="https://github.com/anamankev/brier/security/policy">Security</a></li>
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
            <div className="fine-print">
              <p>
                <strong>Play money only.</strong> Credits come free at sign-up and as a daily allowance. They have no cash
                value and cannot be bought, sold, withdrawn or transferred. 18+.
              </p>
              <p>
                Data from the U.S. Bureau of Labor Statistics, FRED®, the National Weather Service, USGS, NOAA,{" "}
                <a href="https://open-meteo.com/">Open-Meteo.com</a> (CC BY 4.0) and balldontlie. Football data provided by the{" "}
                <a href="https://www.football-data.org/">Football-Data.org</a> API. This product uses the FRED® API but is not
                endorsed or certified by the Federal Reserve Bank of St. Louis; the{" "}
                <a href="https://fred.stlouisfed.org/docs/api/terms_of_use.html">FRED® API Terms of Use</a> apply.
              </p>
            </div>
          </div>
        </footer>
        <CookieBanner />
      </body>
    </html>
  );
}
