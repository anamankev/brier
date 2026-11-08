import type { Metadata } from "next";
import Link from "next/link";
import { me } from "@/lib/server";
import { SignOut } from "@/components/SignOut";
import "./globals.css";

export const metadata: Metadata = {
  title: "Brier",
  description: "A play-money prediction-market exchange with pinned rules, recorded evidence, and scored forecasts.",
};

export default async function RootLayout({ children }: { children: React.ReactNode }) {
  const user = await me();
  return (
    <html lang="en">
      <body>
        <a className="skip" href="#main">
          Skip to content
        </a>
        <header className="top">
          <div className="wrap">
            <Link className="brand" href="/">
              Brier
            </Link>
            <nav className="main" aria-label="Main">
              <Link href="/">Markets</Link>
              <Link href="/leaderboards">Leaderboards</Link>
              {user && <Link href="/portfolio">Portfolio</Link>}
              {user && <Link href="/scorecard">Scorecard</Link>}
              {user?.role === "admin" && <Link href="/admin">Admin</Link>}
            </nav>
            <div className="who">
              {user ? (
                <>
                  <Link href="/account">{user.username}</Link>
                  <SignOut />
                </>
              ) : (
                <>
                  <Link href="/signin">Sign in</Link>
                  <Link href="/register">Create account</Link>
                </>
              )}
            </div>
          </div>
        </header>
        <main id="main" className="wrap">
          {children}
        </main>
        <footer className="wrap muted" style={{ paddingBottom: 32, fontSize: "0.85rem" }}>
          Play money: credits have no cash value and cannot be bought, sold, or withdrawn. Data: U.S. Bureau of
          Labor Statistics, Federal Reserve Bank of St. Louis (FRED®), NOAA NCEI. This product uses the FRED® API but
          is not endorsed or certified by the Federal Reserve Bank of St. Louis. By using FRED® data here you are
          bound by the{" "}
          <a href="https://fred.stlouisfed.org/docs/api/terms_of_use.html">FRED® API Terms of Use</a>.
        </footer>
      </body>
    </html>
  );
}
