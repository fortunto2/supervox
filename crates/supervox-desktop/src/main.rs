#![allow(non_snake_case)]

mod app;
mod audio;

fn main() {
    let _ = dotenvy::dotenv();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("supervox=info".parse().unwrap()),
        )
        .init();

    dioxus::LaunchBuilder::new()
        .with_cfg(
            dioxus::desktop::Config::new()
                .with_window(
                    dioxus::desktop::WindowBuilder::new()
                        .with_title("SuperVox")
                        .with_inner_size(dioxus::desktop::LogicalSize::new(800.0, 500.0))
                        .with_resizable(true),
                )
                .with_disable_context_menu(true),
        )
        .launch(app::App);
}
