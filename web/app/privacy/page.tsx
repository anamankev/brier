import Link from "next/link";
import { TERMS_DATE } from "@/lib/terms";
import { CookieSettingsLink } from "@/components/CookieBanner";

export const metadata = { title: "Privacy policy" };

// Structure after GitHub's Privacy Statement (github/site-policy, CC0 1.0), written for what
// Brier actually holds. Keep it true: every fact here is checked against the code and servers.
const CONTACT = "kanaman@brandeis.edu";

export default function Privacy() {
  return (
    <article className="legal">
      <h1 className="display">Privacy policy</h1>
      <p className="updated">Effective {TERMS_DATE}</p>
      <div className="summary">
        <p>
          Brier keeps the least it needs to run a play-money market: your username, a hash of your password, and your
          trading. No email, no real name, no ads, no analytics, no trackers, and your data is never sold.
        </p>
      </div>

      <h2>Who runs Brier</h2>
      <p>
        Brier is a free, non-commercial project run by Kelvin Anaman, an individual developer in Boston, Massachusetts,
        United States (&ldquo;Brier&rdquo;, &ldquo;we&rdquo;, &ldquo;us&rdquo;). For the purposes of data protection
        law, we are the controller of the personal data described here. Contact details are at the end.
      </p>

      <h2>Personal data we collect</h2>
      <h3>From you</h3>
      <ul>
        <li><b>Account:</b> your username and password. The password is stored only as an Argon2id hash; we never see or keep it in readable form.</li>
        <li><b>Confirmations:</b> when you confirmed you are 18 or older and which version of the terms you accepted, with the time.</li>
        <li><b>Security:</b> hashes of your recovery codes; your two-step sign-in secret, encrypted, if you turn it on; the public half of any API keys you add.</li>
        <li><b>Activity:</b> your orders, trades, positions, balance, daily-allowance claims, and the forecasts you record. These form the exchange&apos;s ledger.</li>
      </ul>
      <h3>Automatically</h3>
      <ul>
        <li><b>Session cookie:</b> a random token that keeps you signed in for up to 24 hours.</li>
        <li><b>Network signals:</b> when you sign up or sign in, a keyed hash of your IP address (not the address itself), used to limit sign-ups per network and to spot one person running several accounts. Refused requests (failed sign-ins, blocked admin calls, rate limits) are logged with the same kind of hash.</li>
        <li><b>Web server logs:</b> like almost every website, our web server records each request: your IP address, the time, the page requested, and your browser&apos;s user agent. Passwords, cookies and session tokens are never written to these logs.</li>
      </ul>
      <h3>From third parties</h3>
      <p>None. We do not buy, receive or combine data about you from anyone else.</p>

      <h2>How we use it</h2>
      <ul>
        <li>To create and run your account, match your orders, settle markets and keep the ledger correct.</li>
        <li>To keep Brier secure and fair: blocking abuse, limiting sign-ups, and detecting wash trading, manipulation and multiple accounts.</li>
        <li>To show the public parts of the game: leaderboards, profiles and market activity.</li>
        <li>To meet legal obligations, if any apply.</li>
      </ul>
      <p>We do not use your data for advertising, profiling, or automated decisions with legal effects, and we do not train AI models on it.</p>

      <h2>Legal bases (EEA and UK)</h2>
      <ul>
        <li><b>Contract:</b> running your account and the markets you trade, under the <Link className="link" href="/terms">terms of service</Link>.</li>
        <li><b>Legitimate interests:</b> security logs, abuse prevention and fair-play checks. These are limited to what protecting the service needs, and you can object (see your rights).</li>
        <li><b>Consent:</b> keeping your watchlist on your device between visits. You can withdraw it at any time in <CookieSettingsLink />.</li>
        <li><b>Legal obligation:</b> where a law requires us to keep or disclose something.</li>
      </ul>

      <h2>What is public</h2>
      <p>
        Your username, your profile, your trades on each market and your scores and rank on leaderboards are visible to
        everyone, as on any exchange. Nothing else about you is shown.
      </p>

      <h2>Sharing</h2>
      <p>We never sell or rent your data, and we do not share it with advertisers or data brokers. It is handled only by:</p>
      <ul>
        <li><b>Oracle Cloud</b> (Ashburn, Virginia, United States), which runs our servers and stores the data on our behalf without using it.</li>
        <li><b>Authorities</b>, only if the law requires it and after checking the request is valid.</li>
      </ul>
      <p>
        Market data comes from public sources (such as the National Weather Service, the Federal Reserve Bank of St.
        Louis and sports data providers). We send them nothing about you.
      </p>

      <h2>International transfers</h2>
      <p>
        Brier&apos;s servers are in the United States. If you use Brier from elsewhere, including the EEA or the UK,
        your data is processed in the United States, which may not have the same data protection laws as your country.
        We protect it as this policy describes wherever you are.
      </p>

      <h2>Retention</h2>
      <ul>
        <li><b>Account data:</b> until you delete your account.</li>
        <li><b>Ledger entries:</b> for as long as the exchange runs, under an anonymous name once the account is deleted, because every trade has a counterparty and the books must balance.</li>
        <li><b>Web server logs:</b> 14 days. <b>Security and system logs:</b> 30 days.</li>
        <li><b>Backups:</b> 30 days, encrypted.</li>
      </ul>

      <h2>Your rights</h2>
      <p>Depending on where you live, you can:</p>
      <ul>
        <li><b>Access and portability:</b> download everything we hold about you as a file from your <Link className="link" href="/account">account page</Link>.</li>
        <li><b>Deletion:</b> delete your account from the same page. Your username, password, recovery codes, two-step secret, API keys and network signals are erased at once; backups age out within 30 days.</li>
        <li><b>Correction, restriction and objection:</b> ask us using the contact below.</li>
        <li><b>Withdraw consent:</b> at any time, in <CookieSettingsLink />.</li>
        <li><b>Complain:</b> to your local data protection authority.</li>
      </ul>
      <p>
        We answer requests within 30 days and never charge for them. Residents of California and other US states with
        privacy laws have the same rights here: we do not sell or share personal information for advertising, and we do
        not treat you differently for using your rights.
      </p>

      <h2>Security</h2>
      <p>
        Passwords are hashed with Argon2id; connections use HTTPS only; two-step sign-in secrets are encrypted; backups
        are encrypted; the database and exchange are not reachable from the internet; and admin actions need two-step
        sign-in and are recorded in an append-only audit log. No system is perfectly secure: if a breach affects your
        data, we will say so on this site as soon as we know. To report a security problem, see our{" "}
        <a className="link" href="https://github.com/anamankev/brier/security/policy">security policy</a>.
      </p>

      <h2>Children</h2>
      <p>
        Brier is only for people 18 or older. We do not knowingly collect data from anyone younger; if we learn an
        account belongs to a minor, we delete it.
      </p>

      <h2>Cookies</h2>
      <p>
        Brier uses one essential cookie for your session and one to remember your cookie choice, and, only if you allow
        it, browser storage to keep your watchlist. There are no analytics, advertising or third-party cookies. Details
        are in the <Link className="link" href="/cookies">cookie policy</Link>.
      </p>

      <h2>Changes</h2>
      <p>
        If this policy changes, the new version and its date appear here. For significant changes, signed-in users see a
        notice and are asked to accept the updated terms before trading again.
      </p>

      <h2>Contact</h2>
      <p>
        Privacy questions and requests: <a className="link" href={`mailto:${CONTACT}`}>{CONTACT}</a>. Please do not post
        personal data in public places such as GitHub issues.
      </p>
    </article>
  );
}
