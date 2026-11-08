import { state } from "./stack";

export default async function teardown(): Promise<void> {
  try {
    process.kill(state().pid, "SIGTERM");
  } catch {
    // already gone
  }
}
