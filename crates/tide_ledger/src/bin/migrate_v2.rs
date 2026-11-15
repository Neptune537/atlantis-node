#[path = "../../examples/migrate_v2.rs"]
mod cli;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    cli::main()
}
