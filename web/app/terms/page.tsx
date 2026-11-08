import Link from "next/link";
import { TERMS_DATE } from "@/lib/terms";

export const metadata = { title: "Terms of use" };

export default function Terms() {
  return (
    <article className="legal">
      <h1 className="display">Terms of use</h1>
      <p className="updated">In effect from {TERMS_DATE}</p>
      <div className="summary">
        <p>
          Brier is a game of forecasting played with play credits. Credits are free, have no cash value, and can never be
          bought, sold, withdrawn, or exchanged for anything. You must be 18 or older.
        </p>
      </div>

      <h2>1. What Brier is</h2>
      <p>
        Brier is a prediction market: you trade contracts on questions such as tomorrow&apos;s high temperature in a city
        or a game&apos;s winner. A contract pays 100 credits if its outcome happens and nothing if it does not. Prices
        come from other players and from an automated market maker run by Brier.
      </p>

      <h2>2. Play credits only</h2>
      <ul>
        <li>Every new account receives 10,000 credits, and accounts can claim a small daily allowance.</li>
        <li>Credits have no monetary value. They cannot be purchased, sold, withdrawn, redeemed, or transferred to another person, inside or outside Brier.</li>
        <li>There are no prizes. Leaderboards and scores are for bragging rights only.</li>
        <li>Do not offer or accept anything of value in exchange for credits, accounts, or trading outcomes.</li>
      </ul>

      <h2>3. Who can play</h2>
      <ul>
        <li>You must be at least 18 years old.</li>
        <li>One account per person. Bots are welcome through API keys, but must belong to a person&apos;s single account and respect the rate limits.</li>
        <li>You are responsible for your password and recovery codes. Brier has no email, so if you lose both, the account cannot be recovered.</li>
      </ul>

      <h2>4. Markets and settlement</h2>
      <ul>
        <li>Each market has a rulebook fixed when it is listed, naming the question, the public source that decides it, and when it settles. You can read every rulebook on its market page.</li>
        <li>Outcomes are decided from the named public source, checked a second time or by a second person before payout. After a result is proposed, holders have a window to dispute it.</li>
        <li>A market can be voided, which returns every trader&apos;s cost, when its source never publishes or the question cannot be answered as written.</li>
        <li>Brier may halt trading in a market, cancel orders, or correct errors to keep the game fair. Decisions on disputes are final.</li>
      </ul>

      <h2>5. Fair play</h2>
      <p>
        You agree to follow the <Link className="link" href="/rules">house rules</Link>. Breaking them can lead to your
        orders being cancelled and your account being closed.
      </p>

      <h2>6. Your content and data</h2>
      <p>
        Your username, trades and scores may appear publicly on leaderboards and market pages. How Brier handles your
        data is described in the <Link className="link" href="/privacy">privacy notice</Link>. You can export your data or
        delete your account at any time from your account page.
      </p>

      <h2>7. No warranty</h2>
      <p>
        Brier is a free, hobby service provided as it is, without guarantees of availability, accuracy, or that markets
        will settle as you expect. Data from public sources can be late or wrong. Nothing on Brier is financial,
        investment, or betting advice.
      </p>

      <h2>8. Limits of liability</h2>
      <p>
        Because credits have no value and the service is free, Brier&apos;s operators are not liable for losses of
        credits, scores, or data, or for any indirect damages arising from your use of Brier, to the extent the law
        allows.
      </p>

      <h2>9. Ending your use</h2>
      <p>
        You can delete your account at any time. Brier can close accounts that break these terms or the house rules, and
        can shut the service down; play credits have no value, so nothing is owed when an account or the service ends.
      </p>

      <h2>10. Changes</h2>
      <p>
        If these terms change, the new version and its date appear here, and you will be asked to accept it before you
        trade again.
      </p>

      <h2>11. Contact</h2>
      <p>
        Questions or reports: open an issue at{" "}
        <a className="link" href="https://github.com/anamankev/brier/issues">
          github.com/anamankev/brier
        </a>
        .
      </p>
    </article>
  );
}
