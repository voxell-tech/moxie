//! The same editor screen, boxed at each row, button and panel.

#[macro_use]
mod common;

screen!(wrap_boxed);

fn main() {
    common::run("boxed", screen::screen());
}
