//! The server's own text in the reader's language: emails, published pages,
//! exports, and the names it gives new things.
//!
//! It shares the web app's catalogs (`web/locales/{code}.json`, from the
//! English text to its translation), so a text is translated once for both.
//! Error messages stay in English here: the app translates them.
//!
//! - Emails use the language the person last used the app in (`PUT
//!   account/language`, sent by the app).
//! - Published pages use the visitor's browser language.
//! - Names of new things (a new account's workspace and guide) use
//!   `MINERVA_LANGUAGE`, or else the language of whoever's request made them.
use super::*;
use std::sync::OnceLock;

/// The languages Minerva has, besides English.
const CATALOGS: &[(&str, &str)] = &[("it", include_str!("../../../web/locales/it.json"))];

fn catalog(language: &str) -> Option<&'static HashMap<String, String>> {
    static PARSED: OnceLock<HashMap<&'static str, HashMap<String, String>>> = OnceLock::new();
    PARSED
        .get_or_init(|| {
            CATALOGS
                .iter()
                .map(|(code, json)| (*code, serde_json::from_str(json).unwrap_or_default()))
                .collect()
        })
        .get(language)
}

/// Marks text for translation where it is kept in a list or chosen by a
/// condition; the place that uses it translates it with `tr`.
pub(crate) const fn msg(text: &'static str) -> &'static str {
    text
}

/// Whether Minerva has `language` (a code such as "it").
pub(crate) fn known(language: &str) -> bool {
    language == "en" || CATALOGS.iter().any(|(code, _)| *code == language)
}

/// `text` in `language`, or as written when there is no translation.
pub(crate) fn tr(language: &str, text: &str) -> String {
    catalog(language)
        .and_then(|c| c.get(text))
        .cloned()
        .unwrap_or_else(|| text.to_string())
}
/// `template` with `{name}` replaced from `values`, in no particular language.
pub(crate) fn fill(template: &str, values: &[(&str, &str)]) -> String {
    values
        .iter()
        .fold(template.to_string(), |out, (name, value)| {
            out.replace(&format!("{{{name}}}"), value)
        })
}
/// `text` in `language`, with `{name}` replaced from `values`.
pub(crate) fn trv(language: &str, text: &str, values: &[(&str, &str)]) -> String {
    values
        .iter()
        .fold(tr(language, text), |out, (name, value)| {
            out.replace(&format!("{{{name}}}"), value)
        })
}

/// The server's language for things made outside anyone's view.
pub(crate) fn default_language() -> String {
    std::env::var("MINERVA_LANGUAGE")
        .ok()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| known(l))
        .unwrap_or_else(|| "en".into())
}

/// The first language Minerva has in an `Accept-Language` header.
pub(crate) fn from_headers(headers: &HeaderMap) -> Option<String> {
    let header = headers.get("accept-language")?.to_str().ok()?;
    let mut tags: Vec<(f32, String)> = header
        .split(',')
        .filter_map(|part| {
            let mut pieces = part.trim().split(';');
            let code = pieces.next()?.trim().split('-').next()?.to_lowercase();
            let q = pieces
                .find_map(|p| p.trim().strip_prefix("q=")?.parse::<f32>().ok())
                .unwrap_or(1.0);
            Some((q, code))
        })
        .collect();
    tags.sort_by(|a, b| b.0.total_cmp(&a.0));
    tags.into_iter().map(|(_, code)| code).find(|c| known(c))
}

/// The language for a visitor: their browser's, else the server's.
pub(crate) fn for_request(headers: &HeaderMap) -> String {
    from_headers(headers).unwrap_or_else(default_language)
}

/// The language a person reads the app in, as it last told the server.
pub(crate) fn of_user(d: &Data, user_id: &str) -> String {
    d.languages
        .get(user_id)
        .filter(|l| known(l))
        .cloned()
        .unwrap_or_else(default_language)
}

/// Text the renderer writes in English, for pages read outside the app.
const RENDERED: &[&str] = &[
    msg("▦ Embedded database · shown to workspace members"),
    msg("▤ Embedded note · shown to workspace members"),
    msg("Reminder by email"),
    msg("Watch on YouTube"),
    msg("YouTube video"),
];
/// Rendered HTML with the renderer's own words in `language`.
pub(crate) fn rendered(language: &str, html: String) -> String {
    if catalog(language).is_none() {
        return html;
    }
    RENDERED.iter().fold(html, |out, text| {
        if out.contains(text) {
            out.replace(text, &tr(language, text))
        } else {
            out
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_from_browsers_and_catalogs() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "accept-language",
            "fr-FR,it;q=0.8,en;q=0.5".parse().unwrap(),
        );
        assert_eq!(from_headers(&headers).as_deref(), Some("it"));
        headers.insert("accept-language", "de".parse().unwrap());
        assert_eq!(from_headers(&headers), None);
        assert_eq!(tr("it", "Cancel"), "Annulla");
        assert_eq!(tr("en", "Cancel"), "Cancel");
        assert_eq!(tr("it", "Not in any catalog"), "Not in any catalog");
        assert_eq!(
            trv("it", "Shared with you by {owner}", &[("owner", "Ada")]),
            "Condivisa con te da Ada"
        );
        assert!(rendered("it", "<p>Reminder by email</p>".into()).contains("Promemoria via email"));
    }
}
