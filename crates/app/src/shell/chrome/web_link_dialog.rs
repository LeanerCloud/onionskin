//! Open Web Link: what the Trust Manager asks before a link opens a web
//! page, unless the user has said to always allow the site, or every site.

use accesskit::Role;
use gpui::{
    div, Context, InteractiveElement as _, IntoElement as _, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};

use super::accessible::{Activation, Element};
use super::{ShellFrame, ThemeTokens};
use crate::preferences::{Preferences, WebLinks};

/// A web link waiting on the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) struct WebLinkPrompt {
    pub(in crate::shell) url: String,
    pub(in crate::shell) host: String,
}

/// What the dialog's buttons do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum WebLinkAction {
    Open,
    AlwaysAllow,
    Cancel,
}

/// What following a web link does, by the Trust Manager's settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum Decision {
    Open,
    Ask,
    Block,
}

/// The Trust Manager's answer for `host`.
pub(in crate::shell) fn decide(preferences: &Preferences, host: &str) -> Decision {
    match preferences.web_links {
        WebLinks::Block => Decision::Block,
        WebLinks::Allow => Decision::Open,
        WebLinks::Ask if preferences.trusted_sites.contains(host) => Decision::Open,
        WebLinks::Ask => Decision::Ask,
    }
}

/// The host a web address names, lower-cased, without a user or a port:
/// what a site is trusted by. Empty for an address with none.
pub(in crate::shell) fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let authority = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    authority
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Said when the Trust Manager blocks web links.
pub(in crate::shell) const BLOCKED: &str =
    "Web links are turned off in Preferences > Trust Manager.";

fn rows(prompt: &WebLinkPrompt) -> Vec<(&'static str, Role, String, Option<WebLinkAction>)> {
    vec![
        (
            "web-link-url",
            Role::Label,
            format!("This document is trying to open {}", prompt.url),
            None,
        ),
        (
            "web-link-open",
            Role::Button,
            "Open".to_owned(),
            Some(WebLinkAction::Open),
        ),
        (
            "web-link-always",
            Role::Button,
            format!("Always Allow {}", prompt.host),
            Some(WebLinkAction::AlwaysAllow),
        ),
        (
            "web-link-cancel",
            Role::Button,
            "Cancel".to_owned(),
            Some(WebLinkAction::Cancel),
        ),
    ]
}

pub(in crate::shell) fn accessible(prompt: &WebLinkPrompt) -> Vec<Element> {
    rows(prompt)
        .into_iter()
        .map(|(id, role, label, action)| {
            let element = Element::new(id, role, label);
            match action {
                Some(action) => element.with_activation(Activation::WebLink(action)),
                None => element,
            }
        })
        .collect()
}

pub(in crate::shell) fn render(
    prompt: &WebLinkPrompt,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut list = div().flex().flex_col().gap_1();
    for (id, _, label, action) in rows(prompt) {
        let row = div().id(id).px_2().py_1().child(label);
        list = list.child(match action {
            Some(action) => row
                .rounded_sm()
                .cursor_pointer()
                .hover(move |row| row.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(Activation::WebLink(action), window, cx);
                }))
                .into_any_element(),
            None => row.text_color(theme.text).into_any_element(),
        });
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_is_the_authority_without_user_or_port() {
        assert_eq!(
            host("https://User@Example.COM:8080/path?q#f"),
            "example.com"
        );
        assert_eq!(host("http://www.example.org"), "www.example.org");
        assert_eq!(host("mailto:someone"), "mailto");
        assert_eq!(host(""), "");
    }

    #[test]
    fn the_trust_manager_decides_by_policy_then_site() {
        let mut preferences = Preferences::default();
        assert_eq!(decide(&preferences, "a.example"), Decision::Ask);
        preferences.trusted_sites.insert("a.example".into());
        assert_eq!(decide(&preferences, "a.example"), Decision::Open);
        assert_eq!(decide(&preferences, "b.example"), Decision::Ask);
        preferences.web_links = WebLinks::Block;
        assert_eq!(
            decide(&preferences, "a.example"),
            Decision::Block,
            "never is never"
        );
        preferences.web_links = WebLinks::Allow;
        assert_eq!(decide(&preferences, "b.example"), Decision::Open);
    }

    #[test]
    fn the_prompt_names_the_address_and_the_site() {
        let prompt = WebLinkPrompt {
            url: "https://a.example/x".into(),
            host: "a.example".into(),
        };
        let labels: Vec<_> = accessible(&prompt).into_iter().map(|e| e.label).collect();
        assert_eq!(
            labels,
            [
                "This document is trying to open https://a.example/x",
                "Open",
                "Always Allow a.example",
                "Cancel"
            ]
        );
    }
}
