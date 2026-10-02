fn main() {
    let spec = robi::web_api::openapi();
    let json = serde_json::to_string_pretty(&spec).expect("failed to serialize OpenAPI spec");
    println!("{json}");
}
