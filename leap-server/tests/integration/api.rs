use crate::utils::{TestResources, TestServer};

use googletest::prelude::*;
use reqwest::StatusCode;

/// Constructs the server and expects it to respond to the health_check API endpoint.
#[tokio::test]
#[gtest]
async fn initialize_server() -> googletest::Result<()> {
    let test_resources = TestResources::try_new_for_test().or_fail()?;
    let server = TestServer::start(&test_resources).or_fail()?;

    let response = reqwest::get(server.endpoint_url("api/health_check")).await;
    expect_that!(
        response,
        ok(property!(&reqwest::Response.status(), eq(StatusCode::OK)))
    );

    Ok(())
}

/// Constructs the server and expects it to respond to the health_check API endpoint.
#[tokio::test]
#[gtest]
async fn version_endpoint() -> googletest::Result<()> {
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
