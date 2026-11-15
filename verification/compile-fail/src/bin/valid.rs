fn main() -> Result<(), String> {
    domain_construction_fixtures::consume(domain_construction_fixtures::verified()?);
    Ok(())
}
