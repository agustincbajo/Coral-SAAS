"use client";

import Link from "next/link";
import { useMe, useRepos, useStartBootstrap, type Repo } from "@/lib/queries";
import { useUI } from "@/lib/store";

export default function ReposPage() {
  const { data: me } = useMe();
  const activeTenantId = useUI((s) => s.activeTenantId);
  const tenantId = activeTenantId ?? me?.tenants[0]?.id;
  const repos = useRepos(tenantId);

  if (!me) {
    return <Skeleton />;
  }

  if (me.tenants.length === 0) {
    return <EmptyTenants />;
  }

  return (
    <div className="max-w-5xl">
      <header className="mb-6 flex items-end justify-between">
        <div>
          <h1 className="text-2xl font-bold">Repositories</h1>
          <p className="text-sm text-gray-600">
            Repos connected through the Coral GitHub App.
          </p>
        </div>
        <a
          href="https://github.com/apps/coral-saas/installations/new"
          target="_blank"
          rel="noopener noreferrer"
          className="rounded bg-gray-900 px-4 py-2 text-sm font-medium text-white hover:bg-gray-800"
        >
          Install GitHub App
        </a>
      </header>

      {repos.isLoading ? (
        <Skeleton />
      ) : repos.error ? (
        <ErrorBlock message={String(repos.error)} />
      ) : repos.data && repos.data.length > 0 ? (
        <RepoTable repos={repos.data} tenantId={tenantId!} />
      ) : (
        <EmptyRepos />
      )}
    </div>
  );
}

function RepoTable({ repos, tenantId }: { repos: Repo[]; tenantId: string }) {
  const bootstrap = useStartBootstrap(tenantId);
  return (
    <div className="overflow-hidden rounded-lg border border-gray-200">
      <table className="w-full text-sm">
        <thead className="bg-gray-50 text-left text-xs uppercase tracking-wide text-gray-500">
          <tr>
            <th className="px-4 py-2">Repository</th>
            <th className="px-4 py-2">Branch</th>
            <th className="px-4 py-2">Status</th>
            <th className="px-4 py-2 text-right">Actions</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-gray-200">
          {repos.map((r) => (
            <tr key={r.id}>
              <td className="px-4 py-3 font-medium">
                <Link
                  href={`/dashboard/repos/${r.id}`}
                  className="hover:text-blue-600"
                >
                  {r.full_name}
                </Link>
              </td>
              <td className="px-4 py-3 text-gray-600">{r.default_branch}</td>
              <td className="px-4 py-3">
                <StatusBadge status={r.bootstrap_status} />
              </td>
              <td className="px-4 py-3 text-right">
                {r.bootstrap_status === "pending" || r.bootstrap_status === "failed" ? (
                  <button
                    type="button"
                    onClick={() => bootstrap.mutate(r.id)}
                    disabled={bootstrap.isPending}
                    className="rounded border border-gray-300 px-3 py-1 text-xs font-medium hover:bg-gray-50 disabled:opacity-50"
                  >
                    {bootstrap.isPending ? "Starting…" : "Run bootstrap"}
                  </button>
                ) : (
                  <Link
                    href={`/dashboard/repos/${r.id}`}
                    className="text-xs text-blue-600 hover:underline"
                  >
                    Open wiki →
                  </Link>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function StatusBadge({ status }: { status: string }) {
  const styles: Record<string, string> = {
    pending: "bg-gray-100 text-gray-700",
    running: "bg-blue-100 text-blue-800",
    ready: "bg-green-100 text-green-800",
    failed: "bg-red-100 text-red-800",
  };
  return (
    <span
      className={
        "inline-flex items-center rounded px-2 py-0.5 text-xs font-medium " +
        (styles[status] ?? "bg-gray-100 text-gray-700")
      }
    >
      {status}
    </span>
  );
}

function Skeleton() {
  return (
    <div className="space-y-2">
      <div className="h-8 w-48 animate-pulse rounded bg-gray-200" />
      <div className="h-32 w-full animate-pulse rounded bg-gray-100" />
    </div>
  );
}

function ErrorBlock({ message }: { message: string }) {
  return (
    <div className="rounded border border-red-200 bg-red-50 p-4 text-sm text-red-700">
      {message}
    </div>
  );
}

function EmptyRepos() {
  return (
    <div className="rounded-lg border border-dashed border-gray-300 bg-gray-50 p-12 text-center">
      <p className="mb-2 text-base font-medium">No repositories yet</p>
      <p className="text-sm text-gray-600">
        Install the Coral GitHub App and grant access to the repos you want a wiki for.
      </p>
    </div>
  );
}

function EmptyTenants() {
  return (
    <div className="rounded-lg border border-dashed border-gray-300 bg-gray-50 p-12 text-center">
      <p className="mb-2 text-base font-medium">No tenant yet</p>
      <p className="text-sm text-gray-600">
        Your tenant is created automatically the first time you install the GitHub App.
      </p>
    </div>
  );
}
