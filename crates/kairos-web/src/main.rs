//! wasm32 entry point (KAIROS-T-0039): trunk compiles this bin; the whole
//! app lives in the lib (`kairos_web::App`) so host builds typecheck it.

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(kairos_web::App)
}
