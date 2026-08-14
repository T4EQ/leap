use crate::utils::{TestResources, TestServer, await_video_downloads};

use googletest::prelude::*;
use reqwest::StatusCode;
use std::sync::LazyLock;
use std::{str::FromStr as _, time::Duration};

static TEST_SECTIONS: LazyLock<Vec<(String, Vec<(String, usize)>)>> = LazyLock::new(|| {
    vec![
        (
            "Section 1".to_owned(),
            vec![
                ("Video 1".to_owned(), 100),
                ("Video 2".to_owned(), 100 * 1024),
            ],
        ),
        (
            "Section 2".to_owned(),
            vec![("Video 3".to_owned(), 200 * 1024 + 10)],
        ),
    ]
});

/// Constructs the server and expects it to respond to the health_check API endpoint.
#[tokio::test]
#[gtest]
async fn initialize_server() -> googletest::Result<()> {
    leap_server::init_logging(None, false).await;

    let test_resources = TestResources::try_new_for_test().or_fail()?;
    let server = TestServer::start(&test_resources).or_fail()?;

    let response = reqwest::get(server.endpoint_url("api/health_check")).await;
    expect_that!(
        response,
        ok(property!(&reqwest::Response.status(), eq(StatusCode::OK)))
    );

    Ok(())
}

/// Queries the version information reported by the server.
#[tokio::test]
#[gtest]
async fn version_endpoint() -> googletest::Result<()> {
    leap_server::init_logging(None, false).await;

    let test_resources = TestResources::try_new_for_test().or_fail()?;
    let server = TestServer::start(&test_resources).or_fail()?;

    let response = reqwest::get(server.endpoint_url("api/version")).await;
    assert_that!(
        response,
        ok(all!(property!(
            &reqwest::Response.status(),
            eq(StatusCode::OK)
        ),))
    );

    let version: leap_api::api::version::get::Response =
        response.or_fail()?.json().await.or_fail()?;
    expect_that!(version.version, eq(std::env!("CARGO_PKG_VERSION")));
    expect_that!(version.name, eq(std::env!("CARGO_PKG_NAME")));
    expect_that!(
        version.authors,
        container_eq(
            std::env!("CARGO_PKG_AUTHORS")
                .split(":")
                .collect::<Vec<_>>()
        )
    );
    expect_that!(version.homepage, eq(std::env!("CARGO_PKG_HOMEPAGE")));
    expect_that!(version.license, eq(std::env!("CARGO_PKG_LICENSE")));
    expect_that!(version.repository, eq(std::env!("CARGO_PKG_REPOSITORY")));

    Ok(())
}

#[tokio::test]
#[gtest]
async fn manifest_and_video_fetching() -> googletest::Result<()> {
    leap_server::init_logging(None, false).await;

    let test_resources = TestResources::try_new_for_test().or_fail()?;

    // Step 1: Save videos and publish a new manifest with them
    let all_videos = test_resources
        .save_videos_and_publish_manifest(
            chrono::NaiveDate::from_str("2026-08-14").or_fail()?,
            &TEST_SECTIONS,
        )
        .await
        .or_fail()?;

    // Step 2: Start the LEAP server
    let server = TestServer::start(&test_resources).or_fail()?;

    // Step 3: Wait for all videos to complete downloading
    await_video_downloads(
        &server,
        all_videos.iter().map(|(v, _)| v),
        Duration::from_secs(5),
    )
    .await
    .or_fail()?;

    // Step 4: Double-check they are actually downloaded
    for (video, data) in &all_videos {
        test_resources
            .verify_saved_video_matches(video, data)
            .await
            .or_fail()?;
    }
    Ok(())
}

#[tokio::test]
#[gtest]
async fn serve_video_content() -> googletest::Result<()> {
    leap_server::init_logging(None, false).await;

    let test_resources = TestResources::try_new_for_test().or_fail()?;

    // Step 1: Save videos and publish a new manifest with them
    let all_videos = test_resources
        .save_videos_and_publish_manifest(
            chrono::NaiveDate::from_str("2026-08-14").or_fail()?,
            &TEST_SECTIONS,
        )
        .await
        .or_fail()?;

    // Step 2: Start the LEAP server
    let server = TestServer::start(&test_resources).or_fail()?;

    // Step 3: Wait for all videos to complete downloading
    await_video_downloads(
        &server,
        all_videos.iter().map(|(v, _)| v),
        Duration::from_secs(5),
    )
    .await
    .or_fail()?;

    // Step 4: Get video data
    for (video, expected_data) in all_videos {
        let endpoint = server.endpoint_url(&format!("api/content/{}", video.id));
        let response = reqwest::get(endpoint).await;
        let response_is_ok = verify_that!(
            response,
            ok(property!(
                &reqwest::Response.status(),
                eq(reqwest::StatusCode::OK)
            ))
        );
        if response_is_ok.is_err() {
            response_is_ok.and_log_failure();
            continue;
        }

        let data = response.unwrap().bytes().await.or_fail()?;
        expect_that!(data, container_eq(expected_data));
    }

    Ok(())
}
