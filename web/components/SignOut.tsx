"use client";
import { useRouter } from "next/navigation";

export function SignOut() {
  const router = useRouter();
  return (
    <button
      type="button"
      onClick={async () => {
        await fetch("/api/session", { method: "DELETE", headers: { "x-brier-request": "1" } });
        router.push("/");
        router.refresh();
      }}
    >
      Sign out
    </button>
  );
}
