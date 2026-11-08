import { redirect } from "next/navigation";
import { gateway, me } from "@/lib/server";
import { AdminConsole, type Entry, type QueueDispute, type QueueResolution } from "./AdminConsole";

export const dynamic = "force-dynamic";
export const metadata = { title: "Admin" };

export default async function Admin() {
  const user = await me();
  if (!user) redirect("/signin");
  if (user.role !== "admin") return <p className="notice error">This page is for admins.</p>;
  if (!user.signed_in_with_totp) {
    return (
      <p className="notice error">
        Admin actions need a sign-in with a TOTP code. <a href="/account">Turn on two-step sign-in</a>, then sign in again.
      </p>
    );
  }
  const [queue, audit] = await Promise.all([
    gateway<{ resolutions: QueueResolution[]; disputes: QueueDispute[] }>("/v1/admin/queue"),
    gateway<{ entries: Entry[] }>("/v1/admin/audit"),
  ]);
  return (
    <div className="stack" style={{ gap: 20 }}>
      <h1 className="display">Admin</h1>
      <p className="muted">Every action here needs a reason and is written to the audit log with your name.</p>
      <AdminConsole
        queue={queue.ok ? queue.body : { resolutions: [], disputes: [] }}
        audit={audit.ok ? audit.body.entries : []}
      />
    </div>
  );
}
