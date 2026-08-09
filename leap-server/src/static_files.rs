use actix_web::web;

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

#[cfg(feature = "provision")]
pub fn register_provisioning_files(app: &mut web::ServiceConfig) {
    let generated = provisioning_files::generate();
    app.service(
        actix_web_static_files::ResourceFiles::new("/", generated).resolve_not_found_to_root(),
    );
}

pub fn register_site_files(app: &mut web::ServiceConfig) {
    let generated = site_files::generate();
    app.service(
        actix_web_static_files::ResourceFiles::new("/", generated).resolve_not_found_to_root(),
    );
}
