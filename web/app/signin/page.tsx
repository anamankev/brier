import { redirect } from "next/navigation";
import { me } from "@/lib/server";
import { SignInForm } from "./SignInForm";

export const dynamic = "force-dynamic";
export const metadata = { title: "Log in" };

export default async function SignIn() {
  if (await me()) redirect("/");
  return <SignInForm />;
}
