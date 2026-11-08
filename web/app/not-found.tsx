import Link from "next/link";

export default function NotFound() {
  return (
    <div className="card auth">
      <h1 className="display">Not found</h1>
      <p className="dim" style={{ margin: 0 }}>There is no market or page at this address.</p>
      <Link href="/" className="btn primary">
        Browse markets
      </Link>
    </div>
  );
}
