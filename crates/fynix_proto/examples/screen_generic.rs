//! An editor screen with every view left as a nested generic type.

#[macro_use]
mod common;

screen!(wrap_none);

fn main() {
    common::run("generic", screen::screen());
}
