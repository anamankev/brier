"use client";
import { useRouter } from "next/navigation";
import { api } from "@/lib/client";

export function CancelButton({ order }: { order: number }) {
  const router = useRouter();
  return (
    <button
      type="button"
      aria-label={`Cancel order ${order}`}
      onClick={async () => {
        await api("DELETE", `/v1/orders/${order}`).catch(() => {});
        router.refresh();
      }}
    >
      Cancel
    </button>
  );
}
