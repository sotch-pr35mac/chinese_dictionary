use chinese_dictionary::query;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let entries = query("西瓜").unwrap_or_default();

    // LexicalUnitRef serializes directly from the compiled archive. No owned
    // LexicalUnit or cloned lexical strings are needed for this JSON response.
    let json = serde_json::to_string(&entries)?;
    println!("{json}");

    Ok(())
}
