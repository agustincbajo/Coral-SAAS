"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";

// The plain /dashboard URL bounces to /dashboard/repos. Kept as its
// own page (rather than a Next.js redirect) because it gives us a
// place to put empty-state UI later (e.g. "you have no tenants yet").
export default function DashboardIndex() {
  const router = useRouter();
  useEffect(() => {
    router.replace("/dashboard/repos");
  }, [router]);
  return null;
}
