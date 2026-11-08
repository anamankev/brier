import { redirect } from "next/navigation";
import { me } from "@/lib/server";
import { RecoverForm } from "./RecoverForm";

export const dynamic = "force-dynamic";
export const metadata = { title: "Reset your password" };

export default async function Recover() {
  if (await me()) redirect("/account");
  return <RecoverForm />;
}
