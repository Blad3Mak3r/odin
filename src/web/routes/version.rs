use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct VersionView {
    pub version: &'static str,
}

pub async fn get_version() -> Json<VersionView> {
    Json(VersionView {
        version: env!("CARGO_PKG_VERSION"),
    })
}
