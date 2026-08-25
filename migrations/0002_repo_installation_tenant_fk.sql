-- Defense-in-depth for tenant isolation: guarantee that a repo's linked
-- GitHub installation belongs to the SAME tenant as the repo.
--
-- Before this, repos.installation_id only REFERENCES github_installations(id),
-- so nothing at the DB layer stopped a repo from pointing at another tenant's
-- installation (which /api/internal/jobs/:id/clone-token would then mint a
-- token for). The control plane now also checks this in code; the composite
-- FK makes it impossible to persist the mismatch in the first place.

-- A composite FK needs a matching unique key on the referenced side.
ALTER TABLE github_installations
    ADD CONSTRAINT github_installations_id_tenant_key UNIQUE (id, tenant_id);

-- Enforce (installation_id, tenant_id) on repos against that key. The
-- existing single-column FK to github_installations(id) stays; this is
-- additive. Cascade matches the existing installation → repos cascade.
ALTER TABLE repos
    ADD CONSTRAINT repos_installation_tenant_fk
    FOREIGN KEY (installation_id, tenant_id)
    REFERENCES github_installations (id, tenant_id)
    ON DELETE CASCADE;
