/**
 * Zustand store for UI-only state. **Never persist sensitive data here**
 * (see CLAUDE.md). Use for: active tenant selection, sidebar collapsed
 * state, toast queue, etc.
 */

import { create } from "zustand";

interface UIState {
  /** Currently selected tenant in the sidebar. null = pick the first. */
  activeTenantId: string | null;
  setActiveTenantId: (id: string | null) => void;

  sidebarCollapsed: boolean;
  toggleSidebar: () => void;
}

export const useUI = create<UIState>((set) => ({
  activeTenantId: null,
  setActiveTenantId: (id) => set({ activeTenantId: id }),

  sidebarCollapsed: false,
  toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
}));
