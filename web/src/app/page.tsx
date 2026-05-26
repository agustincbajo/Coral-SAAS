import { redirect } from "next/navigation";

// The bare `/` route is just a router — if the user has a session
// they land on the dashboard, otherwise on login. The dashboard
// itself handles tenant selection.
export default function HomePage() {
  redirect("/dashboard");
}
