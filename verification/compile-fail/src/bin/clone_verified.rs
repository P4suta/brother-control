fn main() -> Result<(), String> {
    let change = domain_construction_fixtures::verified()?;
    let _copied = change.clone();
    Ok(())
}
