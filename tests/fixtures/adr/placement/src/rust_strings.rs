fn messages() {
    // The strings below reach the built program.
    println!("See ADR-101 for the store.");
    let raw = r#"a raw string naming ADR-102
spanning two lines, still ADR-101"#;
    let multi = "an ordinary string
that spans lines and names ADR-102";
    let code = ADR_101_FLAG; // not a citation: an identifier
}

#[cfg(test)]
mod tests {
    // Test code: comments follow the comment rule, ADR-101 is out of place.
    #[test]
    fn names_record() {
        assert_eq!(super::label(), "ADR-102");
    }
}
