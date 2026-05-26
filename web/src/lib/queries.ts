/**
 * TanStack Query hooks. One per resource. Mutations live alongside
 * the related queries so invalidation is colocated.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "./api-client";

export interface User {
  id: string;
  github_login: string;
  email: string | null;
  avatar_url: string | null;
}

export interface Tenant {
  id: string;
  slug: string;
  name: string;
  plan: string;
}

export interface MeResponse {
  user: User;
  tenants: Tenant[];
}

export interface Repo {
  id: string;
  full_name: string;
  default_branch: string;
  bootstrap_status: string;
  last_indexed_sha: string | null;
}

export interface Job {
  id: string;
  kind: string;
  status: string;
  error: string | null;
  failure_reason: string | null;
  queued_at: string;
  started_at: string | null;
  finished_at: string | null;
}

export const queryKeys = {
  me: ["me"] as const,
  repos: (tenantId: string) => ["tenants", tenantId, "repos"] as const,
  job: (id: string) => ["jobs", id] as const,
};

export function useMe() {
  return useQuery({
    queryKey: queryKeys.me,
    queryFn: () => api.get<MeResponse>("/api/me"),
  });
}

export function useRepos(tenantId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.repos(tenantId ?? ""),
    queryFn: () => api.get<Repo[]>(`/api/tenants/${tenantId}/repos`),
    enabled: Boolean(tenantId),
  });
}

export function useJob(jobId: string | undefined) {
  return useQuery({
    queryKey: queryKeys.job(jobId ?? ""),
    queryFn: () => api.get<Job>(`/api/jobs/${jobId}`),
    enabled: Boolean(jobId),
    // Poll while the job is in flight; back off once terminal.
    refetchInterval: (query) => {
      const data = query.state.data as Job | undefined;
      if (!data) return 2000;
      if (data.status === "queued" || data.status === "running") return 2000;
      return false;
    },
  });
}

export function useStartBootstrap(tenantId: string) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (repoId: string) =>
      api.post<Job>(`/api/tenants/${tenantId}/repos/${repoId}/bootstrap`),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: queryKeys.repos(tenantId) });
    },
  });
}

export function useLogout() {
  return useMutation({
    mutationFn: () => api.post<{ status: string }>("/auth/logout"),
    onSuccess: () => {
      if (typeof window !== "undefined") {
        window.location.href = "/login";
      }
    },
  });
}
