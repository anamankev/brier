import Link from "next/link";
import { TERMS_DATE } from "@/lib/terms";

export const metadata = { title: "House rules" };

export default function Rules() {
  return (
    <article className="legal">
      <h1 className="display">House rules</h1>
      <p className="updated">In effect from {TERMS_DATE}</p>
      <div className="summary">
        <p>Win by forecasting better than everyone else, not by gaming the exchange.</p>
      </div>

      <h2>Play fair</h2>
      <ul>
        <li><b>One account per person.</b> Opening extra accounts to collect credits, climb leaderboards, or trade with yourself is not allowed.</li>
        <li><b>No wash trading.</b> Do not trade with yourself or with accounts you control or coordinate with.</li>
        <li><b>No spoofing.</b> Do not place orders you intend to cancel to move the price.</li>
        <li><b>No manipulation near the close.</b> Do not push a price around just before a market closes to change scores.</li>
        <li><b>Do not trade on outcomes you control.</b> If you can change how a question resolves, do not trade it.</li>
        <li><b>Respect the exchange.</b> Stay within the rate limits, and do not probe for or exploit bugs; report them instead.</li>
      </ul>

      <h2>How it is enforced</h2>
      <p>
        Brier&apos;s surveillance looks for wash trading, spoofing, close manipulation, and accounts that share a network,
        and opens a case for a person to review. When a rule was broken, Brier can cancel your orders and close your
        account. Decisions are recorded in an audit log.
      </p>

      <h2>Bots</h2>
      <p>
        Automated trading is welcome. Create an API key on your account page, sign your requests, and stay within the
        published rate limits. A bot counts as part of its owner&apos;s single account.
      </p>

      <h2>Disputes</h2>
      <p>
        If you hold a position in a market and think its proposed result is wrong, you can dispute it during the dispute
        window shown on the market page. Explain what the rulebook&apos;s source says. An admin rules on every dispute,
        and the ruling is final.
      </p>

      <h2>Usernames</h2>
      <p>Usernames that impersonate others, or that are hateful or obscene, may be closed.</p>

      <p style={{ marginTop: 20 }}>
        See also the <Link className="link" href="/terms">terms of service</Link> and the{" "}
        <Link className="link" href="/privacy">privacy policy</Link>.
      </p>
    </article>
  );
}
