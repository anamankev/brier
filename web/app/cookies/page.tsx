import Link from "next/link";
import { TERMS_DATE } from "@/lib/terms";
import { CookieSettingsLink } from "@/components/CookieBanner";

export const metadata = { title: "Cookie policy" };

// Every cookie and storage item Brier sets. Keep this list complete: the cookie banner promises it.
const ITEMS: { name: string; kind: string; what: string; lasts: string; category: "Essential" | "Preferences" }[] = [
  { name: "__Host-brier_session", kind: "Cookie", what: "Keeps you signed in. httpOnly, so page scripts cannot read it; sent only to this site over HTTPS.", lasts: "24 hours, or until you sign out", category: "Essential" },
  { name: "brier_consent", kind: "Cookie", what: "Remembers your choice in this banner, so it is not shown on every visit.", lasts: "6 months", category: "Essential" },
  { name: "brier.watchlist", kind: "Session storage", what: "The markets you starred, kept for this browsing session only.", lasts: "Until you close the browser", category: "Essential" },
  { name: "brier.watchlist", kind: "Local storage", what: "The markets you starred, kept on this device between visits.", lasts: "Until you clear it or turn Preferences off", category: "Preferences" },
];

export default function Cookies() {
  return (
    <article className="legal">
      <h1 className="display">Cookie policy</h1>
      <p className="updated">Effective {TERMS_DATE}</p>
      <div className="summary">
        <p>
          Brier uses as little browser storage as it can: a cookie to keep you signed in, one to remember your choice,
          and, only if you allow it, your watchlist on this device. No analytics, no advertising, no third-party cookies.
        </p>
      </div>

      <h2>What cookies are</h2>
      <p>
        Cookies are small files a website stores in your browser. Local and session storage are similar browser
        features. This policy covers all of them.
      </p>

      <h2>What Brier stores</h2>
      <div className="table-scroll">
        <table className="cookie-table">
          <caption className="sr-only">Cookies and storage Brier uses</caption>
          <thead>
            <tr>
              <th scope="col">Name</th>
              <th scope="col">Type</th>
              <th scope="col">Purpose</th>
              <th scope="col">Duration</th>
              <th scope="col">Category</th>
            </tr>
          </thead>
          <tbody>
            {ITEMS.map((i) => (
              <tr key={`${i.name}-${i.kind}`}>
                <td><code>{i.name}</code></td>
                <td>{i.kind}</td>
                <td>{i.what}</td>
                <td>{i.lasts}</td>
                <td>{i.category}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <h2>Categories</h2>
      <ul>
        <li><b>Essential:</b> needed for the site to work or for something you asked for (signing in, starring a market for this visit). They are always on and need no consent.</li>
        <li><b>Preferences:</b> keep your watchlist on this device between visits. Off by default until you choose; you can change it any time.</li>
        <li><b>Analytics and advertising:</b> none. Brier does not measure you, profile you or show ads, and loads no third-party scripts, fonts or trackers.</li>
      </ul>

      <h2>Your choices</h2>
      <p>
        Change your choice at any time in <CookieSettingsLink />. Turning Preferences off moves your watchlist to this
        browsing session only. You can also delete cookies and site data in your browser&apos;s settings; you will then
        be signed out and asked again.
      </p>

      <h2>More</h2>
      <p>
        How Brier handles personal data is in the <Link className="link" href="/privacy">privacy policy</Link>.
      </p>
    </article>
  );
}
