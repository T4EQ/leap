use crate::utils::{TestResources, TestServer, await_video_downloads};

use googletest::prelude::*;
use leap_api::types::LocalVideoMeta;
use leap_api::types::VideoStatus;
use reqwest::StatusCode;
use std::sync::LazyLock;
use std::{str::FromStr as _, time::Duration};

type VideoDefinition = (String, usize);
type SectionDefinition = (String, Vec<VideoDefinition>);

static TEST_SECTIONS: LazyLock<Vec<SectionDefinition>> = LazyLock::new(|| {
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

static TEST_SECTIONS_2: LazyLock<Vec<SectionDefinition>> = LazyLock::new(|| {
    vec![
        (
            "Section 3".to_owned(),
            vec![("Video 6".to_owned(), 120 * 1024)],
        ),
        (
            "Section 4".to_owned(),
            vec![
                ("Video 4".to_owned(), 222),
                ("Video 5".to_owned(), 230 * 1024 + 143),
            ],
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
        Duration::from_secs(1),
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
        Duration::from_secs(1),
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

#[tokio::test]
#[gtest]
async fn serve_partial_video_content() -> googletest::Result<()> {
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
        Duration::from_secs(1),
    )
    .await
    .or_fail()?;

    // Step 4: Get video data
    let client = reqwest::Client::new();
    for (video, expected_data) in all_videos {
        let endpoint = server.endpoint_url(&format!("api/content/{}", video.id));
        let request_builder = client.get(endpoint);
        let start = rand::random_range(0..expected_data.len());
        let end = rand::random_range(start..expected_data.len());
        let request_builder = request_builder.header("Range", format!("bytes={start}-{end}"));
        let request = request_builder.build().or_fail()?;
        let response = client.execute(request).await;

        let response_is_ok = verify_that!(
            response,
            ok(property!(
                &reqwest::Response.status(),
                eq(reqwest::StatusCode::PARTIAL_CONTENT)
            ))
        );
        if response_is_ok.is_err() {
            response_is_ok.and_log_failure();
            continue;
        }

        let data = response.unwrap().bytes().await.or_fail()?;
        expect_that!(data, container_eq(expected_data[start..=end].to_vec()));
    }

    Ok(())
}

#[tokio::test]
#[gtest]
async fn fetch_not_downloaded_content() -> googletest::Result<()> {
    leap_server::init_logging(None, false).await;

    let test_resources = TestResources::try_new_for_test().or_fail()?;

    // Step 1: Start the LEAP server
    let server = TestServer::start(&test_resources).or_fail()?;

    // Step 2: Save videos and publish a new manifest with them
    tokio::time::sleep(Duration::from_millis(200)).await;
    let all_videos = test_resources
        .save_videos_and_publish_manifest(
            chrono::NaiveDate::from_str("2026-08-14").or_fail()?,
            &TEST_SECTIONS,
        )
        .await
        .or_fail()?;

    // Step 3: Try to fetch video content
    for (video, _) in all_videos {
        let endpoint = server.endpoint_url(&format!("api/content/{}", video.id));
        let response = reqwest::get(endpoint).await;
        expect_that!(
            response,
            ok(property!(
                &reqwest::Response.status(),
                eq(reqwest::StatusCode::NOT_FOUND)
            ))
        );
    }

    Ok(())
}

#[tokio::test]
#[gtest]
async fn fetch_invalid_video_id() -> googletest::Result<()> {
    leap_server::init_logging(None, false).await;

    let test_resources = TestResources::try_new_for_test().or_fail()?;

    // Step 1: Start the LEAP server
    let server = TestServer::start(&test_resources).or_fail()?;

    let endpoint = server.endpoint_url("api/content/1110-23");
    let response = reqwest::get(endpoint).await;
    expect_that!(
        response,
        ok(property!(
            &reqwest::Response.status(),
            eq(reqwest::StatusCode::BAD_REQUEST)
        ))
    );

    Ok(())
}

#[tokio::test]
#[gtest]
async fn return_video_meta() -> googletest::Result<()> {
    leap_server::init_logging(None, false).await;

    let test_resources = TestResources::try_new_for_test().or_fail()?;
    let manifest_date = chrono::NaiveDate::from_str("2026-08-14").or_fail()?;

    // Step 1: Save videos and publish a new manifest with them
    let all_videos = test_resources
        .save_videos_and_publish_manifest(manifest_date, &TEST_SECTIONS)
        .await
        .or_fail()?;

    // Step 2: Start the LEAP server
    let server = TestServer::start(&test_resources).or_fail()?;

    // Step 3: Wait for all videos to complete downloading
    await_video_downloads(
        &server,
        all_videos.iter().map(|(v, _)| v),
        Duration::from_secs(1),
    )
    .await
    .or_fail()?;

    // Step 4: Get meta
    let endpoint = server.endpoint_url("api/content/meta");
    let response = reqwest::get(endpoint).await;
    verify_that!(
        response,
        ok(property!(
            &reqwest::Response.status(),
            eq(reqwest::StatusCode::OK)
        ))
    )?;

    // Step 5: validate meta
    let response: leap_api::api::content::meta::get::Response =
        response.unwrap().json().await.or_fail()?;
    verify_that!(response.meta, some(anything()))?;
    let meta = response.meta.unwrap();
    expect_eq!(meta.name, "manifest_file");
    expect_eq!(meta.date, manifest_date);

    let validate_videos = |expected_name: &str,
                           expected_size: usize,
                           videos: &[LocalVideoMeta]|
     -> googletest::Result<()> {
        let Some(vid) = videos.iter().find(|v| v.name == expected_name) else {
            Err("Video {expected_name} not found").or_fail()?;
            return Ok(());
        };

        verify_eq!(vid.status, VideoStatus::Downloaded)?;
        verify_eq!(vid.size, expected_size)?;
        verify_eq!(vid.view_count, 0)?;
        Ok(())
    };

    let validate_section =
        |expected_name: &str, expected_content: &[(String, usize)]| -> googletest::Result<()> {
            //
            let section = meta
                .content
                .iter()
                .find(|section| *expected_name == section.name);
            let Some(section) = section else {
                Err("Section {expected_name} not found").or_fail()?;
                return Ok(());
            };

            for (video_name, video_len) in expected_content {
                validate_videos(video_name, *video_len, &section.content)?;
            }
            Ok(())
        };

    for (section_name, section_content) in &*TEST_SECTIONS {
        validate_section(section_name, section_content).and_log_failure();
    }

    Ok(())
}

#[tokio::test]
#[gtest]
async fn return_specific_video_meta() -> googletest::Result<()> {
    leap_server::init_logging(None, false).await;

    let test_resources = TestResources::try_new_for_test().or_fail()?;
    let manifest_date = chrono::NaiveDate::from_str("2026-08-14").or_fail()?;

    // Step 1: Save videos and publish a new manifest with them
    let all_videos = test_resources
        .save_videos_and_publish_manifest(manifest_date, &TEST_SECTIONS)
        .await
        .or_fail()?;

    // Step 2: Start the LEAP server
    let server = TestServer::start(&test_resources).or_fail()?;

    // Step 3: Wait for all videos to complete downloading
    await_video_downloads(
        &server,
        all_videos.iter().map(|(v, _)| v),
        Duration::from_secs(1),
    )
    .await
    .or_fail()?;

    for (video, data) in all_videos {
        // Step 4: Get meta
        let endpoint = server.endpoint_url(&format!("api/content/meta/{}", video.id));
        let response = reqwest::get(endpoint).await;
        verify_that!(
            response,
            ok(property!(
                &reqwest::Response.status(),
                eq(reqwest::StatusCode::OK)
            ))
        )?;

        // Step 5: validate meta
        let response: leap_api::api::content::meta::id::get::Response =
            response.unwrap().json().await.or_fail()?;

        verify_that!(
            response.meta,
            some(all!(
                field!(LocalVideoMeta.name, eq(&video.name)),
                field!(LocalVideoMeta.id, eq(&video.id.to_string())),
                field!(&LocalVideoMeta.size, eq(data.len())),
                field!(LocalVideoMeta.status, pat!(VideoStatus::Downloaded)),
                field!(&LocalVideoMeta.view_count, eq(0)),
            ))
        )?;
    }

    Ok(())
}

#[tokio::test]
#[gtest]
async fn view_count_increments() -> googletest::Result<()> {
    leap_server::init_logging(None, false).await;

    let test_resources = TestResources::try_new_for_test().or_fail()?;
    let manifest_date = chrono::NaiveDate::from_str("2026-08-14").or_fail()?;

    // Step 1: Save videos and publish a new manifest with them
    let all_videos = test_resources
        .save_videos_and_publish_manifest(manifest_date, &TEST_SECTIONS)
        .await
        .or_fail()?;

    // Step 2: Start the LEAP server
    let server = TestServer::start(&test_resources).or_fail()?;

    // Step 3: Wait for all videos to complete downloading
    await_video_downloads(
        &server,
        all_videos.iter().map(|(v, _)| v),
        Duration::from_secs(1),
    )
    .await
    .or_fail()?;

    let all_videos: Vec<_> = all_videos
        .into_iter()
        .zip([12_u64, 4, 5])
        .map(|((video, data), count)| (video, data, count))
        .collect();

    // Step 4: Increment view counts
    let client = reqwest::Client::new();
    for (video, _, count) in &all_videos {
        for _ in 0..*count {
            let endpoint = server.endpoint_url(&format!("api/content/{}/view", video.id));
            let request = client.post(endpoint).build().or_fail()?;
            let response = client.execute(request).await;
            verify_that!(
                response,
                ok(property!(
                    &reqwest::Response.status(),
                    eq(reqwest::StatusCode::OK)
                ))
            )?;
        }
    }

    // Step 5: Validate view counts
    for (video, data, expected_view_counts) in all_videos {
        let endpoint = server.endpoint_url(&format!("api/content/meta/{}", video.id));
        let response = reqwest::get(endpoint).await;
        verify_that!(
            response,
            ok(property!(
                &reqwest::Response.status(),
                eq(reqwest::StatusCode::OK)
            ))
        )?;

        let response: leap_api::api::content::meta::id::get::Response =
            response.unwrap().json().await.or_fail()?;

        verify_that!(
            response.meta,
            some(all!(
                field!(LocalVideoMeta.name, eq(&video.name)),
                field!(LocalVideoMeta.id, eq(&video.id.to_string())),
                field!(&LocalVideoMeta.size, eq(data.len())),
                field!(LocalVideoMeta.status, pat!(VideoStatus::Downloaded)),
                field!(&LocalVideoMeta.view_count, eq(expected_view_counts)),
            ))
        )?;
    }

    Ok(())
}

#[tokio::test]
#[gtest]
async fn manifest_updates_after_first_sync() -> googletest::Result<()> {
    leap_server::init_logging(None, false).await;

    let test_resources = TestResources::try_new_for_test().or_fail()?;
    let first_manifest_date = chrono::NaiveDate::from_str("2026-08-14").or_fail()?;

    // Step 1: Save videos and publish a new manifest with them
    let first_manifest_videos = test_resources
        .save_videos_and_publish_manifest(first_manifest_date, &TEST_SECTIONS)
        .await
        .or_fail()?;

    // Step 2: Start the LEAP server
    let server = TestServer::start(&test_resources).or_fail()?;

    // Step 3: Wait for all videos to complete downloading
    await_video_downloads(
        &server,
        first_manifest_videos.iter().map(|(v, _)| v),
        Duration::from_secs(1),
    )
    .await
    .or_fail()?;

    // Step 4: Set new manifest
    let second_manifest_date = chrono::NaiveDate::from_str("2026-08-15").or_fail()?;
    let second_manifest_videos = test_resources
        .save_videos_and_publish_manifest(second_manifest_date, &TEST_SECTIONS_2)
        .await
        .or_fail()?;

    // Step 5: Wait for download. Manifest is not immediately downloaded, should time out.
    let result = await_video_downloads(
        &server,
        second_manifest_videos.iter().map(|(v, _)| v),
        Duration::from_secs(1),
    )
    .await;
    verify_that!(result, err(anything()))?;

    // Step 6: Trigger manifest update
    let client = reqwest::Client::new();
    let endpoint = server.endpoint_url("api/manifest/fetch");
    let request = client.post(endpoint).build().or_fail()?;
    let response = client.execute(request).await;
    verify_that!(
        response,
        ok(property!(
            &reqwest::Response.status(),
            eq(reqwest::StatusCode::OK)
        ))
    )?;

    // Step 6: Wait for download. Now it should be successful
    await_video_downloads(
        &server,
        second_manifest_videos.iter().map(|(v, _)| v),
        Duration::from_secs(1),
    )
    .await?;

    Ok(())
}

#[tokio::test]
#[gtest]
async fn manifest_does_not_update_with_same_date() -> googletest::Result<()> {
    leap_server::init_logging(None, false).await;

    let test_resources = TestResources::try_new_for_test().or_fail()?;
    let first_manifest_date = chrono::NaiveDate::from_str("2026-08-14").or_fail()?;

    // Step 1: Save videos and publish a new manifest with them
    let first_manifest_videos = test_resources
        .save_videos_and_publish_manifest(first_manifest_date, &TEST_SECTIONS)
        .await
        .or_fail()?;

    // Step 2: Start the LEAP server
    let server = TestServer::start(&test_resources).or_fail()?;

    // Step 3: Wait for all videos to complete downloading
    await_video_downloads(
        &server,
        first_manifest_videos.iter().map(|(v, _)| v),
        Duration::from_secs(1),
    )
    .await
    .or_fail()?;

    // Step 4: Set new manifest
    let second_manifest_date = chrono::NaiveDate::from_str("2026-08-14").or_fail()?;
    let second_manifest_videos = test_resources
        .save_videos_and_publish_manifest(second_manifest_date, &TEST_SECTIONS_2)
        .await
        .or_fail()?;

    // Step 5: Wait for download. Manifest is not immediately downloaded, should time out.
    let result = await_video_downloads(
        &server,
        second_manifest_videos.iter().map(|(v, _)| v),
        Duration::from_secs(1),
    )
    .await;
    verify_that!(result, err(anything()))?;

    // Step 6: Trigger manifest update
    let client = reqwest::Client::new();
    let endpoint = server.endpoint_url("api/manifest/fetch");
    let request = client.post(endpoint).build().or_fail()?;
    let response = client.execute(request).await;
    verify_that!(
        response,
        ok(property!(
            &reqwest::Response.status(),
            eq(reqwest::StatusCode::OK)
        ))
    )?;

    // Step 7: Wait for download. Manifest is not downloaded, should time out.
    let result = await_video_downloads(
        &server,
        second_manifest_videos.iter().map(|(v, _)| v),
        Duration::from_secs(1),
    )
    .await;
    verify_that!(result, err(anything()))?;

    Ok(())
}

#[tokio::test]
#[gtest]
async fn fetch_current_manifest() -> googletest::Result<()> {
    leap_server::init_logging(None, false).await;

    let test_resources = TestResources::try_new_for_test().or_fail()?;
    let first_manifest_date = chrono::NaiveDate::from_str("2026-08-14").or_fail()?;

    // Step 1: Save videos and publish a new manifest with them
    let first_manifest_videos = test_resources
        .save_videos_and_publish_manifest(first_manifest_date, &TEST_SECTIONS)
        .await
        .or_fail()?;

    // Step 2: Start the LEAP server
    let server = TestServer::start(&test_resources).or_fail()?;

    // Step 3: Wait for all videos to complete downloading
    await_video_downloads(
        &server,
        first_manifest_videos.iter().map(|(v, _)| v),
        Duration::from_secs(1),
    )
    .await
    .or_fail()?;

    // Step 4: Fetch manifest
    let endpoint = server.endpoint_url("api/manifest/latest");
    let response = reqwest::get(endpoint).await;
    verify_that!(
        response,
        ok(property!(
            &reqwest::Response.status(),
            eq(reqwest::StatusCode::OK)
        ))
    )?;

    // Step 5: Validate manifest
    let manifest_received = response.unwrap().text().await;
    let manifest_on_disk =
        &tokio::fs::read_to_string(test_resources.file_server_path().join("manifest.json"))
            .await
            .or_fail()?;
    expect_that!(&manifest_received, ok(eq(manifest_on_disk)));

    Ok(())
}
