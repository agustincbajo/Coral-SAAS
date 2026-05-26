"use client";

import { use } from "react";
import { useMe, useRepos } from "@/lib/queries";
import { useUI } from "@/lib/store";

export default function RepoDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = use(params);
  const { data: me } = useMe();
  const activeTenantId = useUI((s) => s.activeTenantId);
  const tenantId = activeTenantId ?? me?.tenants[0]?.id;
  const repos = useRepos(tenantId);

  const repo = repos.data?.find((r) => r.id === id);

  if (!me || repos.isLoading) {
    return <div className="text-sm text-gray-500">Loading…</div>;
  }
  if (!repo) {
    return (
      <div className="rounded border border-amber-200 bg-amber-50 p-4 text-sm text-amber-800">
        Repository not found, or you don&apos;t have access.
      </div>
    );
  }

  return (
    <div className="max-w-4xl">
      <header className="mb-6">
        <h1 className="text-2xl font-bold">{repo.full_name}</h1>
        <p className="text-sm text-gray-600">
          Branch <code>{repo.default_branch}</code> · Status{" "}
          <code>{repo.bootstrap_status}</code>
        </p>
      </header>

      {repo.bootstrap_status === "ready" ? (
        <WikiPlaceholder />
      ) : repo.bootstrap_status === "running" ? (
        <div className="rounded border border-blue-200 bg-blue-50 p-4 text-sm text-blue-800">
          Bootstrap in progress…
        </div>
      ) : (
        <div className="rounded border border-gray-200 bg-gray-50 p-4 text-sm text-gray-700">
          No wiki yet for this repo. Trigger a bootstrap from the repos list.
        </div>
      )}
    </div>
  );
}

function WikiPlaceholder() {
  return (
    <div className="prose max-w-none">
      <p className="text-gray-600">
        Wiki rendering is wired in Fase 4 (markdown from R2 →
        sanitize → render). For now, you would see the repo&apos;s wiki here.
      </p>
    </div>
  );
}
