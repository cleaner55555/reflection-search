//! Minimalni `robots.txt` parser: naš UA + `*`, `Disallow` prefixi.
//!
//! Nepoznate direktive se ignorišu. Neuspeh skidanja = sve dozvoljeno.

/// Pravila jednog domena.
#[derive(Debug, Clone, Default)]
pub struct Rules {
    disallow: Vec<String>,
}

impl Rules {
    /// Prazna pravila — sve dozvoljeno.
    #[must_use]
    pub fn allow_all() -> Self {
        Self::default()
    }

    /// Parsira `robots.txt`. Gleda grupe za našeg bota i `*`.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut disallow = Vec::new();
        let mut in_scope = false;
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            match key.trim().to_lowercase().as_str() {
                "user-agent" => {
                    let ua = value.trim().to_lowercase();
                    in_scope =
                        ua == "*" || ua.contains("reflection-search") || ua.contains("reflection");
                }
                "disallow" => {
                    if in_scope {
                        let path = value.trim();
                        if !path.is_empty() {
                            disallow.push(path.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
        Self { disallow }
    }

    /// Da li je putanja dozvoljena (prefix match).
    #[must_use]
    pub fn allows(&self, path: &str) -> bool {
        !self.disallow.iter().any(|d| path.starts_with(d.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_means_allow() {
        assert!(Rules::allow_all().allows("/anything"));
        assert!(Rules::parse("").allows("/anything"));
    }

    #[test]
    fn star_group_applies() {
        let r = Rules::parse("User-agent: *\nDisallow: /private/\n");
        assert!(!r.allows("/private/x"));
        assert!(r.allows("/public"));
    }

    #[test]
    fn other_bot_group_ignored() {
        let r = Rules::parse("User-agent: googlebot\nDisallow: /\n");
        assert!(r.allows("/"));
    }

    #[test]
    fn our_group_applies() {
        let r = Rules::parse("User-agent: reflection-search-bot\nDisallow: /tmp/\n");
        assert!(!r.allows("/tmp/a"));
        assert!(r.allows("/ok"));
    }

    #[test]
    fn comments_and_case_ignored() {
        let r = Rules::parse("# komentar\nUSER-AGENT: * # kraj\nDISALLOW: /x/ # kraj\n");
        assert!(!r.allows("/x/y"));
        assert!(r.allows("/y"));
    }
}
