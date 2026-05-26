"use client";

import { useLogout, useMe } from "@/lib/queries";

export function Topbar() {
  const { data } = useMe();
  const logout = useLogout();

  if (!data) return <div className="h-14 border-b border-gray-200" />;

  const { user } = data;
  const initial = user.github_login?.charAt(0).toUpperCase() ?? "?";

  return (
    <header className="flex h-14 items-center justify-end gap-4 border-b border-gray-200 bg-white px-6">
      <div className="flex items-center gap-3 text-sm">
        {user.avatar_url ? (
          // Plain img — Next/Image would need remotePatterns config. Avatar size is tiny.
          // eslint-disable-next-line @next/next/no-img-element
          <img
            src={user.avatar_url}
            alt={user.github_login}
            className="h-8 w-8 rounded-full"
          />
        ) : (
          <div className="flex h-8 w-8 items-center justify-center rounded-full bg-gray-300 text-sm font-semibold text-gray-700">
            {initial}
          </div>
        )}
        <span className="text-gray-700">{user.github_login}</span>
        <button
          type="button"
          onClick={() => logout.mutate()}
          disabled={logout.isPending}
          className="rounded border border-gray-300 px-3 py-1 text-xs font-medium text-gray-700 hover:bg-gray-50 disabled:opacity-50"
        >
          {logout.isPending ? "Signing out…" : "Sign out"}
        </button>
      </div>
    </header>
  );
}
