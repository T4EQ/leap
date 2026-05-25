//! LEAP Provisioning Site.
//!
//! This crate provides the web-based user interface for the LEAP provisioning process.
//! It allows users to configure storage, network, and LEAP-specific settings
//! via a browser.
//!
//! The application is built using [Yew](https://yew.rs/).

pub mod app;

mod completed;
mod leap_config;
mod network_config;
mod storage_config;
mod utils;
