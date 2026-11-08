/** A change in percentage points with its direction in text, not only colour. */
export function Delta({ change, suffix = "" }: { change: number | null; suffix?: string }) {
  if (change == null) return null;
  if (change === 0) return <span className="delta flat">0{suffix}</span>;
  const up = change > 0;
  return (
    <span className={`delta ${up ? "up" : "down"}`}>
      <span aria-hidden="true">{up ? "▲" : "▼"}</span>
      <span className="sr-only">{up ? "up" : "down"}</span> {Math.abs(change)}
      {suffix}
    </span>
  );
}
