//! Exercises the theme engine without wiring it into the binary: the module
//! is included by path, so its unit tests run here.

#[path = "../src/themes/mod.rs"]
mod themes;
