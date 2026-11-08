import { redirect } from "next/navigation";
import { me } from "@/lib/server";
import { TotpSetup } from "./TotpSetup";

export const dynamic = "force-dynamic";

export default async function Account() {
  const user = await me();
  if (!user) redirect("/signin");
  return (
    <>
      <h1>{user.username}</h1>
      <p className="muted">Role: {user.role}</p>
      <section className="card" aria-labelledby="totp-h">
        <h2 id="totp-h">Two-step sign-in</h2>
        {user.totp ? (
          <p>On. Signing in asks for a code from your authenticator app.</p>
        ) : (
          <>
            <p>Protect your account with a code from an authenticator app. Admin accounts must have it.</p>
            <TotpSetup />
          </>
        )}
        {user.role === "admin" && !user.signed_in_with_totp && (
          <p className="notice error">Admin actions need a sign-in with a code: sign out and sign in again.</p>
        )}
      </section>
    </>
  );
}
