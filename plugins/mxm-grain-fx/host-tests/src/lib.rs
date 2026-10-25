//! mxm-grain-fx hosted by MXM Player. The tests are in `tests/` and load the plugin's release bundle
//! from `target/bundled`, skipping with the command to build it when it is missing:
//! `cargo xtask bundle mxm-grain-fx --release` and `cargo xtask fetch`, then
//! `cargo test -p mxm-grain-fx-host-tests`.
