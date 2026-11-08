//! Compiles the Slint UI in `ui/`.
// A UI that fails to compile must abort the build, so `expect` is the right tool here.
#![allow(clippy::expect_used)]

fn main() {
    slint_build::compile("ui/app.slint").expect("failed to compile the Slint UI");
}
