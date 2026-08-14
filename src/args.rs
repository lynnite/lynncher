pub fn parse_initial_uri() -> Option<String> {
    std::env::args().nth(1).filter(|s| !s.trim().is_empty())
}
