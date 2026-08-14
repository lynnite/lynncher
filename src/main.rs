mod app;
mod args;
mod backend;
mod bootstrap;

fn main() -> eframe::Result<()> {
    bootstrap::run(args::parse_initial_uri())
}
