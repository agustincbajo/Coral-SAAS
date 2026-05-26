"use client";

import { use } from "react";
import { ApiError } from "@/lib/api-client";
import { useMe, useWikiPage } from "@/lib/queries";
import { useUI } from "@/lib/store";

export default function WikiPage({
  params,
}: {
  params: Promise<{ id: string; slug: string }>;
}) {
  const { id, slug } = use(params);
  const { data: me } = useMe();
  const activeTenantId = useUI((s) => s.activeTenantId);
  const tenantId = activeTenantId ?? me?.tenants[0]?.id;

  const page = useWikiPage(tenantId, id, slug);

  if (page.isLoading) {
    return <div className="text-sm text-gray-500">Loading…</div>;
  }

  if (page.error) {
    const status = page.error instanceof ApiError ? page.error.status : 0;
    if (status === 404) {
      return (
        <div className="rounded border border-amber-200 bg-amber-50 p-4 text-sm text-amber-800">
          Wiki page <code>{slug}</code> not found yet. If the bootstrap
          just finished, give it a moment and refresh.
        </div>
      );
    }
    return (
      <div className="rounded border border-red-200 bg-red-50 p-4 text-sm text-red-700">
        Failed to load wiki page: {String(page.error)}
      </div>
    );
  }

  if (!page.data) return null;

  return (
    <article className="prose prose-slate max-w-3xl">
      {page.data.title && (
        <h1 className="mb-6 text-3xl font-bold">{page.data.title}</h1>
      )}
      {/* Server-rendered + sanitized via ammonia in api/src/wiki/render.rs.
          Safe to inject as HTML — no untrusted script can reach here. */}
      <div
        className="wiki-rendered"
        // eslint-disable-next-line react/no-danger
        dangerouslySetInnerHTML={{ __html: page.data.html }}
      />
    </article>
  );
}
