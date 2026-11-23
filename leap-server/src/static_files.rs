use actix_web::{HttpResponse, http::header, web};

mod site_files {
    #![allow(
        clippy::all,
        clippy::pedantic,
        clippy::nursery,
        unused_imports,
        unused_variables
    )]
    include!(concat!(env!("OUT_DIR"), "/site/generated.rs"));
}

mod provisioning_files {
    #![allow(
        clippy::all,
        clippy::pedantic,
        clippy::nursery,
        unused_imports,
        unused_variables,
        dead_code
    )]
    include!(concat!(env!("OUT_DIR"), "/provisioning/generated.rs"));
}

fn register_static_files(
    app: &mut web::ServiceConfig,
    mut resources: std::collections::HashMap<&'static str, static_files::Resource>,
) {
    const INDEX_PAGE: &str = "index.html";
    if let Some(static_files::Resource { data, .. }) = resources.remove(INDEX_PAGE) {
        let handler = move || async move {
            HttpResponse::Ok()
                .insert_header((header::CACHE_CONTROL, "no-store"))
                .content_type("text/html; charset=utf-8")
                .body(data)
        };

        app.service(web::resource(INDEX_PAGE).get(handler));
        app.default_service(web::get().to(handler));
    }

    app.service(
        actix_web_static_files::ResourceFiles::new("/", resources).skip_handler_when_not_found(),
    );
}

#[cfg(feature = "provision")]
pub fn register_provisioning_files(app: &mut web::ServiceConfig) {
    let generated = provisioning_files::generate();
    register_static_files(app, generated);
}

pub fn register_site_files(app: &mut web::ServiceConfig) {
    let generated = site_files::generate();
    register_static_files(app, generated);
}

#[cfg(test)]
mod tests {
    use actix_web::{App, http::Method};
    use googletest::prelude::*;

    use super::*;

    async fn assert_frontend_cache_policy(
        configure: fn(&mut web::ServiceConfig),
        mut paths: Vec<String>,
    ) {
        let app = actix_web::test::init_service(App::new().configure(configure)).await;

        paths.extend(["/", "/unknown"].map(str::to_owned));
        for path in paths.into_iter().filter(|p| !p.starts_with("/.")) {
            let request = actix_web::test::TestRequest::default()
                .method(Method::GET)
                .uri(&path)
                .to_request();
            let response = actix_web::test::call_service(&app, request).await;
            assert!(response.status().is_success(), "{path}");
            let is_not_cached = ["/", "/index.html", "/unknown"].contains(&path.as_str());
            if is_not_cached {
                let expected = "no-store";
                assert_eq!(
                    response.headers().get(header::CACHE_CONTROL).unwrap(),
                    expected,
                    "{path}",
                );
            } else {
                expect_eq!(response.headers().get(header::CACHE_CONTROL), None);
            }
        }
    }

    #[gtest]
    #[actix_web::test]
    async fn site_responses_have_correct_cache_policy() {
        let paths = site_files::generate()
            .keys()
            .map(|path| format!("/{path}"))
            .collect();
        assert_frontend_cache_policy(register_site_files, paths).await;
    }

    #[cfg(feature = "provision")]
    #[gtest]
    #[actix_web::test]
    async fn provisioning_responses_have_correct_cache_policy() {
        let paths = provisioning_files::generate()
            .keys()
            .map(|path| format!("/{path}"))
            .collect();
        assert_frontend_cache_policy(register_provisioning_files, paths).await;
    }
}
