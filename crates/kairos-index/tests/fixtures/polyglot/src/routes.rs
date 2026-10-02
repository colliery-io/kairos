//! 2 routes to one function, for `path` (COLLIERY-T-2531).

/// Takes the short route or the long route.
pub fn dispatch(full: bool) -> u32 {
    if full {
        direct()
    } else {
        indirect()
    }
}

/// The short route.
fn direct() -> u32 {
    target()
}

/// The long route, through `relay`.
fn indirect() -> u32 {
    relay()
}

/// The second step of the long route.
fn relay() -> u32 {
    target()
}

/// The function that both routes reach.
pub fn target() -> u32 {
    42
}
