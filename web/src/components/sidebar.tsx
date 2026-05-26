"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useMe } from "@/lib/queries";
import { useUI } from "@/lib/store";

export function Sidebar() {
  const { data, isLoading } = useMe();
  const activeTenantId = useUI((s) => s.activeTenantId);
  const setActiveTenantId = useUI((s) => s.setActiveTenantId);
  const pathname = usePathname();

  // Default active tenant to the first one once /api/me resolves.
  const tenants = data?.tenants ?? [];
  const resolvedTenant = tenants.find((t) => t.id === activeTenantId) ?? tenants[0];

  return (
    <aside className="flex h-screen w-64 flex-col border-r border-gray-200 bg-gray-50">
      <div className="flex h-14 items-center border-b border-gray-200 px-4">
        <Link href="/dashboard" className="text-lg font-bold">
          Coral
        </Link>
      </div>

      <div className="flex-1 overflow-y-auto p-4 space-y-6">
        <section>
          <h2 className="mb-2 text-xs font-semibold uppercase tracking-wide text-gray-500">
            Tenant
          </h2>
          {isLoading ? (
            <div className="text-sm text-gray-400">Loading…</div>
          ) : tenants.length === 0 ? (
            <div className="text-sm text-gray-500">No tenants yet</div>
          ) : (
            <select
              value={resolvedTenant?.id ?? ""}
              onChange={(e) => setActiveTenantId(e.target.value)}
              className="w-full rounded border border-gray-300 bg-white px-2 py-1.5 text-sm"
            >
              {tenants.map((t) => (
                <option key={t.id} value={t.id}>
                  {t.name} · {t.plan}
                </option>
              ))}
            </select>
          )}
        </section>

        <nav className="space-y-1">
          <NavLink
            href="/dashboard/repos"
            label="Repositories"
            active={pathname?.startsWith("/dashboard/repos") ?? false}
          />
          <NavLink
            href="/dashboard/settings"
            label="Settings"
            active={pathname === "/dashboard/settings"}
          />
        </nav>
      </div>
    </aside>
  );
}

function NavLink({
  href,
  label,
  active,
}: {
  href: string;
  label: string;
  active: boolean;
}) {
  return (
    <Link
      href={href}
      className={
        "block rounded px-2 py-1.5 text-sm transition " +
        (active
          ? "bg-gray-900 text-white"
          : "text-gray-700 hover:bg-gray-200")
      }
    >
      {label}
    </Link>
  );
}
