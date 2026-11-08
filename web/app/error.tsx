"use client";

export default function ErrorPage({ reset }: { error: Error; reset: () => void }) {
  return (
    <div className="card auth" role="alert">
      <h1 className="display">Something went wrong</h1>
      <p className="dim" style={{ margin: 0 }}>
        The page could not be shown. Your orders and balances are safe: they live in the exchange, not in this page.
      </p>
      <button type="button" className="btn primary" onClick={reset}>
        Try again
      </button>
    </div>
  );
}
