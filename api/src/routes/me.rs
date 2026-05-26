//! `/api/me` — info about the authenticated user, including which
//! tenants they belong to.

use crate::{
    auth::AuthUser,
    db::models::{Tenant, User},
    error::ApiResult,
    state::AppState,
};
use axum::{extract::State, routing::get, Json, Router};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct MeResponse {
    user: UserDto,
    tenants: Vec<TenantDto>,
}

#[derive(Debug, Serialize)]
struct UserDto {
    id: uuid::Uuid,
    github_login: String,
    email: Option<String>,
    avatar_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct TenantDto {
    id: uuid::Uuid,
    slug: String,
    name: String,
    plan: String,
}

async fn me(State(app): State<AppState>, user: AuthUser) -> ApiResult<Json<MeResponse>> {
    let u = User::get_by_id(app.db(), user.user_id).await?;
    let tenants = Tenant::list_for_user(app.db(), user.user_id).await?;

    Ok(Json(MeResponse {
        user: UserDto {
            id: u.id,
            github_login: u.github_login,
            email: u.email,
            avatar_url: u.avatar_url,
        },
        tenants: tenants
            .into_iter()
            .map(|t| TenantDto {
                id: t.id,
                slug: t.slug,
                name: t.name,
                plan: t.plan,
            })
            .collect(),
    }))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/me", get(me))
}
