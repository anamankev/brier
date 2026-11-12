import Link from "next/link";
import { TERMS_DATE } from "@/lib/terms";

export const metadata = { title: "Terms of service" };

// Structure after GitHub's Terms of Service (github/site-policy, CC0 1.0), written for a free,
// play-money prediction market.
const CONTACT = "kanaman@brandeis.edu";

export default function Terms() {
  return (
    <article className="legal">
      <h1 className="display">Terms of service</h1>
      <p className="updated">Effective {TERMS_DATE}</p>
      <div className="summary">
        <p>
          Brier is a forecasting game played with free play credits. Credits have no cash value and can never be bought,
          sold, withdrawn or exchanged for anything. You must be 18 or older. Play fairly, keep your account to
          yourself, and remember the service is free and comes without guarantees.
        </p>
      </div>

      <h2>1. Agreement</h2>
      <p>
        These terms are an agreement between you and Kelvin Anaman, who runs Brier (&ldquo;Brier&rdquo;,
        &ldquo;we&rdquo;, &ldquo;us&rdquo;). By creating an account or using the site or its API, you accept them,
        together with the <Link className="link" href="/rules">house rules</Link>, the{" "}
        <Link className="link" href="/privacy">privacy policy</Link> and the <Link className="link" href="/cookies">cookie policy</Link>.
        If you do not agree, do not use Brier.
      </p>

      <h2>2. Definitions</h2>
      <ul>
        <li><b>Credits:</b> the play units used on Brier. 100 credits is the payout of one winning contract.</li>
        <li><b>Market:</b> a yes-or-no question with a rulebook that says how and when it is decided.</li>
        <li><b>Contract:</b> a position in a market that pays 100 credits if its outcome happens and nothing if it does not.</li>
        <li><b>Rulebook:</b> the fixed rules of a market: its question, its public source, its schedule and how it settles.</li>
      </ul>

      <h2>3. Play credits only</h2>
      <ul>
        <li>New accounts receive 10,000 credits, and every account can claim the same small allowance once a day.</li>
        <li>Credits have no monetary value. They cannot be purchased, sold, withdrawn, redeemed or transferred, inside or outside Brier, and Brier has no way to pay for or with them.</li>
        <li>There are no prizes. Leaderboards and scores are for recognition only.</li>
        <li>Do not offer, ask for or accept anything of value in exchange for credits, accounts, positions or outcomes.</li>
        <li>Brier is not gambling, a financial service or an exchange for real assets, and nothing on it is financial, investment or betting advice.</li>
      </ul>

      <h2>4. Accounts</h2>
      <ul>
        <li>You must be at least 18 years old, and use of Brier must be lawful where you live.</li>
        <li>One account per person. Bots are welcome through API keys, but each must belong to a person&apos;s single account.</li>
        <li>Choose a username that does not impersonate anyone, mislead, or offend; we may rename or close accounts that do.</li>
        <li>You are responsible for everything done with your account. Keep your password, recovery codes and API keys secret, and turn on two-step sign-in. Brier has no email, so if you lose both your password and your recovery codes, the account cannot be recovered.</li>
      </ul>

      <h2>5. Markets and settlement</h2>
      <ul>
        <li>Each market&apos;s rulebook is fixed when it is listed and can be read on its page. Its fingerprint is written to the exchange&apos;s log, so it cannot change once trading starts.</li>
        <li>Outcomes are decided from the public source the rulebook names, and checked a second time, or by a person, before payout. After a result is proposed, holders have a window to dispute it.</li>
        <li>A market is voided, paying 50 credits per contract to each side, if its source never publishes or the question cannot be answered as written.</li>
        <li>To keep the game fair, we may halt a market, cancel orders, reverse trades caused by a clear error, or correct a settlement. Decisions on disputes are final.</li>
        <li>Prices come from other players and from an automated market maker that Brier runs with the same rules and limits as everyone else.</li>
      </ul>

      <h2>6. Acceptable use</h2>
      <p>You agree not to:</p>
      <ul>
        <li>run several accounts, trade with yourself, or coordinate with others to manipulate prices, scores or leaderboards;</li>
        <li>exploit bugs, errors or delays in data instead of reporting them;</li>
        <li>attack, overload, probe or scrape Brier beyond its published API and rate limits, or try to reach data that is not yours;</li>
        <li>use Brier for anything unlawful, or to harass anyone.</li>
      </ul>
      <p>The <Link className="link" href="/rules">house rules</Link> give the details. Breaking them can lead to cancelled orders or a closed account.</p>

      <h2>7. The API</h2>
      <p>
        You may use the public REST and WebSocket APIs within their documented rate limits. Requests are signed with keys
        you create; you are responsible for them. We may change or limit the API to protect the service.
      </p>

      <h2>8. Intellectual property</h2>
      <ul>
        <li>The Brier name, design and software belong to us. The source code is published for reading, not reuse, as its licence says.</li>
        <li>Market data belongs to its sources and is used under their terms, credited in the footer and in the data-sources notes.</li>
        <li>You keep ownership of anything you write (such as your username). You let us show it on Brier as part of running the service.</li>
      </ul>

      <h2>9. Third-party data</h2>
      <p>
        Brier relies on public data that can be late, revised or wrong. We settle by the rulebook and the source&apos;s
        first publication as the rulebook defines it, even if the source later changes its figures.
      </p>

      <h2>10. Disclaimer of warranties</h2>
      <p>
        Brier is provided &ldquo;as is&rdquo; and &ldquo;as available&rdquo;, without warranties of any kind, including
        availability, accuracy, or fitness for a purpose. We do not promise that the service will be uninterrupted,
        secure or error-free, or that markets will settle as you expect.
      </p>

      <h2>11. Limitation of liability</h2>
      <p>
        Credits have no value and the service is free. To the fullest extent the law allows, we are not liable for any
        loss of credits, positions, scores or data, or for any indirect, incidental or consequential damages arising from
        your use of Brier. Nothing in these terms limits liability that cannot be limited by law.
      </p>

      <h2>12. Indemnity</h2>
      <p>
        If you break these terms or the law while using Brier and someone makes a claim against us because of it, you
        agree to cover the reasonable costs that result, to the extent the law allows.
      </p>

      <h2>13. Ending your use</h2>
      <p>
        You can delete your account at any time from your <Link className="link" href="/account">account page</Link>. We
        can suspend or close accounts that break these terms or the house rules, and we can change or shut down the
        service. Because credits have no value, nothing is owed when an account or the service ends. Sections 3, 8 and 10
        to 15 continue to apply afterwards.
      </p>

      <h2>14. Changes to these terms</h2>
      <p>
        We may update these terms. The new version and its date appear here, and signed-in users are asked to accept it
        before they place another order. If you do not accept, you can keep viewing Brier or delete your account.
      </p>

      <h2>15. General</h2>
      <ul>
        <li><b>Governing law:</b> these terms are governed by the laws of the Commonwealth of Massachusetts, United States, without regard to conflict-of-law rules, except where the law of your country gives you protections that cannot be waived.</li>
        <li><b>Severability:</b> if a part of these terms cannot be enforced, the rest still applies.</li>
        <li><b>No waiver:</b> not enforcing a term once does not give it up.</li>
        <li><b>Assignment:</b> you may not transfer your account or these terms; we may transfer them to whoever runs Brier next.</li>
        <li><b>Entire agreement:</b> these terms, the house rules, the privacy policy and the cookie policy are the whole agreement between you and us about Brier.</li>
      </ul>

      <h2>16. Contact</h2>
      <p>
        Questions about these terms: <a className="link" href={`mailto:${CONTACT}`}>{CONTACT}</a>. Bugs and suggestions:{" "}
        <a className="link" href="https://github.com/anamankev/brier/issues">GitHub issues</a>. Security problems: our{" "}
        <a className="link" href="https://github.com/anamankev/brier/security/policy">security policy</a>.
      </p>
    </article>
  );
}
