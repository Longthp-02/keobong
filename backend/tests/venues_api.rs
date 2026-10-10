//! Acceptance tests for the venue list hosts choose from.

mod common;

use axum::http::StatusCode;
use axum::http::header::CACHE_CONTROL;
use common::*;
use serde_json::json;
use sqlx::PgPool;

#[sqlx::test]
async fn lists_the_active_launch_venues(pool: PgPool) {
    let app = app(pool.clone());
    sqlx::query("UPDATE venues SET active = false WHERE slug = 'khu-the-thao-an-phu'")
        .execute(&pool)
        .await
        .unwrap();

    let response = call(&app, get("/api/venues")).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[CACHE_CONTROL], "public, max-age=300");
    assert_eq!(
        json_body(response).await,
        json!({
            "venues": [
                { "id": "ssa-amitie", "name": "SSA Sports Center (Amitie Thảo Điền)", "address": "28 Duyên Hải, An Khánh" },
                { "id": "an-phu-nguyen-hoang", "name": "Sân bóng An Phú Quận 2", "address": "93 Nguyễn Hoàng, Bình Trưng" }
            ]
        })
    );
}
