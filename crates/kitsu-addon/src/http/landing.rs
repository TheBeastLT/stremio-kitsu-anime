pub const LANDING_HTML: &str = include_str!("landing.html");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::manifest;

    #[test]
    fn landing_names_the_manifest_version() {
        let manifest = manifest();
        assert!(LANDING_HTML.contains(&format!("<h1 class=\"name\">{}</h1>", manifest.name)));
        assert!(LANDING_HTML.contains(&format!("<h2 class=\"version\">v{}</h2>", manifest.version)));
    }
}
