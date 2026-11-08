import Link from "next/link";
import { TERMS_DATE } from "@/lib/terms";

export const metadata = { title: "Privacy notice" };

export default function Privacy() {
  return (
    <article className="legal">
      <h1 className="display">Privacy notice</h1>
      <p className="updated">In effect from {TERMS_DATE}</p>
      <div className="summary">
        <p>
          Brier keeps the minimum it needs to run the game: your username, a hash of your password, and your trading.
          No email, no real name, no ads, no trackers, no selling of data.
        </p>
      </div>

      <h2>What Brier keeps</h2>
      <ul>
        <li><b>Account:</b> your username; your password, stored only as an Argon2 hash; when you confirmed you are 18 or older and accepted the terms; hashes of your recovery codes; your authenticator secret, encrypted, if you turn on two-step sign-in.</li>
        <li><b>Trading:</b> your orders, trades, positions, balance, and the forecasts you record. These are part of the exchange&apos;s ledger.</li>
        <li><b>Network signals:</b> when you sign up or log in, a keyed hash of your IP address (not the address itself). It is used only to limit sign-ups per network and to detect one person running several accounts.</li>
        <li><b>One cookie:</b> your session, needed to keep you logged in. There are no analytics or advertising cookies.</li>
      </ul>

      <h2>What is public</h2>
      <p>
        Your username, your trades on each market, and your scores and rank on leaderboards are visible to everyone,
        as on any exchange. Nothing else is shown.
      </p>

      <h2>Why it is kept</h2>
      <p>
        To run your account and the markets you trade, to keep the game fair (detecting wash trading, manipulation, and
        multiple accounts), and to keep the exchange&apos;s records consistent. Brier does not use your data for anything
        else and never sells or shares it.
      </p>

      <h2>Who else sees it</h2>
      <p>
        Brier runs on servers operated by its owner on a cloud provider, which stores the data but does not use it.
        Market data comes from public sources; Brier sends them nothing about you.
      </p>

      <h2>Your choices</h2>
      <ul>
        <li><b>Export:</b> download everything Brier holds about you, as a file, from your <Link className="link" href="/account">account page</Link>.</li>
        <li><b>Delete:</b> delete your account from the same page. Your username, password, codes, and network signals are erased at once. Your past trades stay in the ledger under an anonymous name, because each trade has a counterparty and the exchange&apos;s books must balance.</li>
      </ul>

      <h2>How long it is kept</h2>
      <p>
        Account data is kept until you delete the account. Anonymous ledger entries are kept for as long as the exchange
        runs. Backups are kept for up to 30 days.
      </p>

      <h2>Contact</h2>
      <p>
        Questions or requests: open an issue at{" "}
        <a className="link" href="https://github.com/anamankev/brier/issues">
          github.com/anamankev/brier
        </a>
        .
      </p>
    </article>
  );
}
