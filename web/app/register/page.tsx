import { redirect } from "next/navigation";
import { me } from "@/lib/server";
import { RegisterForm } from "./RegisterForm";

export const dynamic = "force-dynamic";
export const metadata = { title: "Create your account" };

export default async function Register() {
  if (await me()) redirect("/");
  return <RegisterForm />;
}
