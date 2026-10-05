# Code convention

## Confirm before reaching for memory-management types

`Arc`, `Rc`, `Box<dyn Trait>`, `Mutex`, `RwLock`, `Cell`, `RefCell`, or
any other memory-management/interior-mutability type: stop and confirm
with the user before introducing one. They carry a real runtime cost -
an allocation, a reference count, a lock - and are easy to reach for
out of habit where plain ownership or borrowing would have worked;
overused, they add up and slow the program down. Explain what plain
ownership or borrowing was tried first and why it didn't work, and let
the user decide rather than reaching for one of these as the default
fix.

## Turbofish, not an annotated binding

When a generic call's type needs pinning down, prefer turbofish on
the call itself (`.collect::<Vec<_>>()`, `.parse::<i32>()`, and so on)
over annotating the binding's type to steer inference.

## Test our own code, and only what can break

A test covers logic written here. Bevy and the other crates we build
on have their own tests: do not assert that a `clamp` clamps, that a
reflected value survives a save, or that a component is where it was
just inserted.

Skip a test that only restates the line it covers: a `match` written
out again as a table of asserts, a flag set from `is_some()`, a
one-line formula. If the test would change in step with every edit of
the code and catch nothing else, it is not worth having.

Keep tests where a mistake is easy to make and hard to see in review:
maths with a sign or a space to get wrong, a round trip through our
own format, a behaviour a user asked for by name.

When it's a toss-up whether a test earns its place, leave it out.
