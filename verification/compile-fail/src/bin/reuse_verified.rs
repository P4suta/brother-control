fn main() -> Result<(), String> {
    let change = domain_construction_fixtures::verified()?;
    domain_construction_fixtures::consume(change);
    domain_construction_fixtures::consume(change);
    Ok(())
}
