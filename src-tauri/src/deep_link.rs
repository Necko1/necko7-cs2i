pub fn normalize(code: &str) -> Result<String, String> {
    let code = code.trim().to_ascii_uppercase();
    let raw = if code.len() == 9 && code.as_bytes()[4] == b'-' {
        code.replace('-', "")
    } else {
        code
    };
    if raw.len() != 8
        || !raw
            .bytes()
            .all(|b| b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789".contains(&b))
    {
        return Err("Enter a valid eight-character pairing code".into());
    }
    Ok(format!("{}-{}", &raw[..4], &raw[4..]))
}
pub fn parse(input: &str) -> Result<String, String> {
    if input.len() > 128 {
        return Err("Pairing link is too long".into());
    }
    let url = url::Url::parse(input).map_err(|_| "Invalid pairing link")?;
    let params: Vec<_> = url.query_pairs().collect();
    if url.scheme() != "necko7-cs2i"
        || url.host_str() != Some("pair")
        || !url.path().is_empty()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
        || params.len() != 1
        || params[0].0 != "code"
    {
        return Err("Invalid pairing link".into());
    }
    normalize(&params[0].1)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn links() {
        assert_eq!(
            parse("necko7-cs2i://pair?code=abcd-2345").unwrap(),
            "ABCD-2345"
        );
        for input in [
            "https://pair?code=ABCD2345",
            "necko7-cs2i://evil?code=ABCD2345",
            "necko7-cs2i://pair?code=ABCD2345&x=1",
            "necko7-cs2i://pair?code=ABCD2345&code=ABCD2345",
            "necko7-cs2i://pair?code=ABCD0123",
            "necko7-cs2i://pair/path?code=ABCD2345",
            "necko7-cs2i://user@pair?code=ABCD2345",
        ] {
            assert!(parse(input).is_err(), "{input}");
        }
    }
}
