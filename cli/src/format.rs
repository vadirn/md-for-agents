use std::str::FromStr;

/// Two-variant output format: what a human reads, or what a program parses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextJson {
    Text,
    Json,
}

impl FromStr for TextJson {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" => Ok(TextJson::Text),
            "json" => Ok(TextJson::Json),
            _ => Err(format!("unknown format: {} (expected text or json)", s)),
        }
    }
}

impl std::fmt::Display for TextJson {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TextJson::Text => write!(f, "text"),
            TextJson::Json => write!(f, "json"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_roundtrip() {
        assert_eq!(TextJson::from_str("text").unwrap(), TextJson::Text);
        assert_eq!(TextJson::from_str("json").unwrap(), TextJson::Json);
        assert_eq!(TextJson::from_str("JSON").unwrap(), TextJson::Json);
        assert!(TextJson::from_str("yaml").is_err());
        assert_eq!(TextJson::Text.to_string(), "text");
        assert_eq!(TextJson::Json.to_string(), "json");
    }
}
