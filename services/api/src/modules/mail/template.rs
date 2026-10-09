//! Email templates in `templates/`: each email is an HTML and a plain-text content file wrapped
//! in the shared `layout.html` / `layout.txt` (header and footer). Placeholders are `{{name}}`;
//! values are HTML-escaped in the HTML part and never rescanned, so a value cannot inject markup
//! or another placeholder. An unknown or unfilled placeholder fails rendering.

use lettre::{
    Message,
    message::{Mailbox, MultiPart},
};

const LAYOUT_HTML: &str = include_str!("templates/layout.html");
const LAYOUT_TEXT: &str = include_str!("templates/layout.txt");

/// One email's subject and content files.
#[derive(Clone, Copy, Debug)]
pub struct Template {
    pub subject: &'static str,
    html: &'static str,
    text: &'static str,
}

pub const VERIFY_EMAIL: Template = Template {
    subject: "Verify your SceneCask email",
    html: include_str!("templates/verify_email.html"),
    text: include_str!("templates/verify_email.txt"),
};

pub const PASSWORD_RESET_EMAIL: Template = Template {
    subject: "Reset your SceneCask password",
    html: include_str!("templates/password_reset.html"),
    text: include_str!("templates/password_reset.txt"),
};

/// A rendered email, ready to address.
#[derive(Debug)]
pub struct Rendered {
    pub subject: &'static str,
    pub html: String,
    pub text: String,
}

impl Template {
    /// Renders the content with `values`, then wraps it in the layout, which also receives
    /// `origin` (the public web origin) and `subject`.
    pub fn render(&self, origin: &str, values: &[(&str, &str)]) -> Result<Rendered, &'static str> {
        let html = fill(self.html, values, |_, value| escape_html(value))?;
        let text = fill(self.text, values, |_, value| value.to_owned())?;
        let layout = |content| {
            [
                ("content", content),
                ("origin", origin),
                ("subject", self.subject),
            ]
        };
        Ok(Rendered {
            subject: self.subject,
            // The content is already rendered and escaped; only the layout's own values escape.
            html: fill(LAYOUT_HTML, &layout(html.trim_end()), |name, value| {
                if name == "content" {
                    value.to_owned()
                } else {
                    escape_html(value)
                }
            })?,
            text: fill(LAYOUT_TEXT, &layout(text.trim_end()), |_, value| {
                value.to_owned()
            })?,
        })
    }
}

impl Rendered {
    pub fn into_message(self, from: Mailbox, to: Mailbox) -> Result<Message, &'static str> {
        Message::builder()
            .from(from)
            .to(to)
            .subject(self.subject)
            .multipart(MultiPart::alternative_plain_html(self.text, self.html))
            .map_err(|_| "message_build")
    }
}

/// Replaces each `{{name}}` with `encode(name, value)` in a single pass over `template`.
fn fill(
    template: &str,
    values: &[(&str, &str)],
    encode: impl Fn(&str, &str) -> String,
) -> Result<String, &'static str> {
    substitute(template, |name| {
        values
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(key, value)| encode(key, value))
    })
}

/// Single pass over `template`, replacing each `{{name}}` with `lookup(name)`.
fn substitute(
    template: &str,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<String, &'static str> {
    let mut output = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        output.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find("}}").ok_or("template_unclosed_placeholder")?;
        output.push_str(&lookup(after[..end].trim()).ok_or("template_unknown_placeholder")?);
        rest = &after[end + 2..];
    }
    output.push_str(rest);
    Ok(output)
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: &str = "https://scenecask.example";

    #[test]
    fn verify_email_renders_inside_the_shared_layout() {
        let link = "https://scenecask.example/auth/verify#token=abc_-123";
        let rendered = VERIFY_EMAIL.render(ORIGIN, &[("link", link)]).unwrap();
        for part in [&rendered.html, &rendered.text] {
            assert!(part.contains(link));
            assert!(part.contains("expires in 24 hours"));
            // Footer from the layout, with the origin filled in.
            assert!(part.contains(&format!("SceneCask account at {ORIGIN}.")));
            assert!(!part.contains("{{"));
        }
        assert!(rendered.html.starts_with("<!doctype html>"));
        assert!(
            rendered
                .html
                .contains(&format!("<title>{}</title>", VERIFY_EMAIL.subject))
        );
        assert!(rendered.html.contains(&format!("href=\"{link}\"")));
        assert!(!rendered.text.contains('<'));
    }

    #[test]
    fn password_reset_email_states_its_lifetime() {
        let link = "https://scenecask.example/auth/reset/confirm#token=abc_-123";
        let rendered = PASSWORD_RESET_EMAIL
            .render(ORIGIN, &[("link", link)])
            .unwrap();
        for part in [&rendered.html, &rendered.text] {
            assert!(part.contains(link));
            assert!(part.contains("expires in 30 minutes"));
            assert!(part.contains("signs you out everywhere"));
            assert!(!part.contains("{{"));
        }
        assert_eq!(rendered.subject, "Reset your SceneCask password");
    }

    #[test]
    fn values_are_escaped_and_never_rescanned() {
        let rendered = VERIFY_EMAIL
            .render(ORIGIN, &[("link", "\"><script>{{origin}}</script>")])
            .unwrap();
        assert!(
            rendered
                .html
                .contains("&quot;&gt;&lt;script&gt;{{origin}}&lt;/script&gt;")
        );
        assert!(!rendered.html.contains("<script>"));
        assert!(
            rendered.text.contains("{{origin}}"),
            "values are inserted verbatim"
        );
    }

    #[test]
    fn missing_or_malformed_placeholders_fail() {
        assert_eq!(
            VERIFY_EMAIL.render(ORIGIN, &[]).unwrap_err(),
            "template_unknown_placeholder"
        );
        assert_eq!(
            substitute("a {{b", |_| None).unwrap_err(),
            "template_unclosed_placeholder"
        );
    }

    #[test]
    fn builds_a_multipart_message() {
        let message = VERIFY_EMAIL
            .render(ORIGIN, &[("link", "https://scenecask.example/x")])
            .unwrap()
            .into_message(
                "SceneCask <no-reply@localhost>".parse().unwrap(),
                "ana@example.test".parse().unwrap(),
            )
            .unwrap();
        let formatted = String::from_utf8(message.formatted()).unwrap();
        assert!(formatted.contains("multipart/alternative"));
        assert!(formatted.contains("text/plain"));
        assert!(formatted.contains("text/html"));
        assert!(formatted.contains("Subject: Verify your SceneCask email"));
    }
}
