//! Usenet backbones: which servers read from the same article store.
//!
//! The bundled list (`data/backbones.yaml`, from usenet.rexum.space) maps each
//! backbone to the providers that sell access to it, each provider to its
//! website domain, and a few confirmed NNTP host names to their backbone.
//!
//! `ServerN.Backbone` names the backbone of a server. Servers with the same
//! name are copies of each other: an article one of them answers `430` for is
//! not asked of the others (see `nntp::BackboneMiss`). The name is interned to
//! the server's `group` id, which the server pool, the article downloader and
//! the duplicate health checks already use; ids stay stable for the life of
//! the process, so a reload never points an in-flight article at another
//! backbone.

use std::fmt::Write as _;
use std::sync::{Mutex, OnceLock};

use yaml_rust2::{Yaml, YamlLoader};

const DATA: &str = include_str!("../data/backbones.yaml");

/// One backbone and the providers that sell access to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backbone {
    pub name: String,
    pub providers: Vec<String>,
}

/// The parsed bundled list.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Catalog {
    pub backbones: Vec<Backbone>,
    /// provider -> website domain (lowercase)
    pub domains: Vec<(String, String)>,
    /// confirmed NNTP host (lowercase) -> backbone
    pub hosts: Vec<(String, String)>,
}

/// How a host name relates to the bundled list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchKind {
    /// The host is a confirmed NNTP host: its backbone can be filled in.
    Confirmed,
    /// The host is under a provider's website domain: its backbones are a
    /// suggestion the user confirms.
    Suggested,
    /// Nothing known about the host.
    Unknown,
    /// An IP address, `localhost` or a name without a dot (a local proxy, for
    /// example): nothing to look up.
    Skipped,
}

impl MatchKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MatchKind::Confirmed => "confirmed",
            MatchKind::Suggested => "suggested",
            MatchKind::Unknown => "unknown",
            MatchKind::Skipped => "skipped",
        }
    }
}

/// The result of [`lookup`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lookup {
    pub kind: MatchKind,
    pub provider: Option<String>,
    /// Backbones the host may read from, in list order. One for a confirmed
    /// host; one or more for a suggestion.
    pub backbones: Vec<String>,
}

fn yaml_str(y: &Yaml) -> Option<String> {
    match y {
        Yaml::String(s) => Some(s.trim().to_owned()),
        Yaml::Integer(i) => Some(i.to_string()),
        Yaml::Real(r) => Some(r.clone()),
        _ => None,
    }
    .filter(|s| !s.is_empty())
}

fn str_map(y: &Yaml, lower_key: bool, lower_value: bool) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    match y {
        Yaml::Hash(h) => {
            for (k, v) in h {
                let (Some(k), Some(v)) = (yaml_str(k), yaml_str(v)) else {
                    return Err(format!("bad entry {k:?}: {v:?}"));
                };
                out.push((
                    if lower_key { k.to_ascii_lowercase() } else { k },
                    if lower_value { v.to_ascii_lowercase() } else { v },
                ));
            }
        }
        Yaml::Null | Yaml::BadValue => {}
        _ => return Err("expected a map".into()),
    }
    Ok(out)
}

/// Parses the backbone list format (see `data/backbones.yaml`).
pub fn parse(text: &str) -> Result<Catalog, String> {
    let docs = YamlLoader::load_from_str(text).map_err(|e| e.to_string())?;
    let Some(doc) = docs.first() else {
        return Ok(Catalog::default());
    };
    let mut catalog = Catalog::default();
    match &doc["backbones"] {
        Yaml::Array(items) => {
            for item in items {
                let Yaml::Hash(h) = item else {
                    return Err("each backbone must be `- Name: [providers]`".into());
                };
                for (k, v) in h {
                    let name = yaml_str(k).ok_or("backbone without a name")?;
                    let providers = match v {
                        Yaml::Array(list) => list.iter().filter_map(yaml_str).collect(),
                        Yaml::Null => Vec::new(),
                        _ => return Err(format!("providers of {name} must be a list")),
                    };
                    catalog.backbones.push(Backbone { name, providers });
                }
            }
        }
        Yaml::BadValue | Yaml::Null => {}
        _ => return Err("`backbones` must be a list".into()),
    }
    catalog.domains = str_map(&doc["domains"], false, true)?;
    catalog.hosts = str_map(&doc["hosts"], true, false)?;
    Ok(catalog)
}

/// The bundled list. A broken file is a build mistake; the unit tests parse it.
pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| parse(DATA).unwrap_or_default())
}

/// The form two backbone names are compared in: trimmed, inner spaces
/// collapsed, case ignored.
pub fn normalize(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A name as it is stored: trimmed with spaces collapsed, and spelled as in the
/// bundled list when it names a known backbone.
pub fn display_name(name: &str) -> String {
    let key = normalize(name);
    if key.is_empty() {
        return String::new();
    }
    catalog()
        .backbones
        .iter()
        .find(|b| normalize(&b.name) == key)
        .map(|b| b.name.clone())
        .unwrap_or_else(|| name.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn is_ip_literal(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    h.parse::<std::net::IpAddr>().is_ok()
}

impl Catalog {
    fn backbones_of(&self, provider: &str) -> Vec<String> {
        let key = normalize(provider);
        self.backbones
            .iter()
            .filter(|b| b.providers.iter().any(|p| normalize(p) == key))
            .map(|b| b.name.clone())
            .collect()
    }

    /// What is known about a news server host name.
    pub fn lookup(&self, host: &str) -> Lookup {
        let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
        let none = |kind| Lookup {
            kind,
            provider: None,
            backbones: Vec::new(),
        };
        if host.is_empty() || is_ip_literal(&host) || !host.contains('.') || host == "localhost" {
            return none(MatchKind::Skipped);
        }
        if let Some((_, backbone)) = self.hosts.iter().find(|(h, _)| *h == host) {
            let provider = self
                .domains
                .iter()
                .find(|(_, d)| host == *d || host.ends_with(&format!(".{d}")))
                .map(|(p, _)| p.clone());
            return Lookup {
                kind: MatchKind::Confirmed,
                provider,
                backbones: vec![display_name(backbone)],
            };
        }
        // the longest matching domain wins (news.usenet.farm is Usenet.Farm, not a "farm" provider)
        let best = self
            .domains
            .iter()
            .filter(|(_, d)| host == *d || host.ends_with(&format!(".{d}")))
            .max_by_key(|(_, d)| d.len());
        match best {
            Some((provider, _)) => {
                let backbones = self.backbones_of(provider);
                if backbones.is_empty() {
                    return none(MatchKind::Unknown);
                }
                Lookup {
                    kind: MatchKind::Suggested,
                    provider: Some(provider.clone()),
                    backbones,
                }
            }
            None => none(MatchKind::Unknown),
        }
    }
}

fn interned() -> &'static Mutex<Vec<String>> {
    static NAMES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    NAMES.get_or_init(|| Mutex::new(Vec::new()))
}

/// The id servers on backbone `name` share (1 and up), or 0 for an empty name
/// (an independent server). The same name always gets the same id while the
/// process runs.
pub fn intern(name: &str) -> i32 {
    let key = normalize(name);
    if key.is_empty() {
        return 0;
    }
    let Ok(mut names) = interned().lock() else {
        return 0;
    };
    if let Some(pos) = names.iter().position(|n| normalize(n) == key) {
        return pos as i32 + 1;
    }
    names.push(display_name(name));
    names.len() as i32
}

/// The name behind an id from [`intern`].
pub fn name_of(id: i32) -> Option<String> {
    if id <= 0 {
        return None;
    }
    interned().lock().ok()?.get(id as usize - 1).cloned()
}

/// The backbone of a server from its options: `Backbone` wins; without it (or
/// with it empty) an old `Group` above 0 is read as the custom backbone
/// "Group N", so servers that shared a group still count as copies of each
/// other. Settings drops `Group` when it saves, so an emptied Backbone stays empty.
pub fn server_backbone(backbone: Option<&[u8]>, group: Option<&[u8]>) -> String {
    let named = backbone.map(|b| display_name(&String::from_utf8_lossy(b))).unwrap_or_default();
    if !named.is_empty() {
        return named;
    }
    let group = group.map_or(0, crate::feed::c_atoi_bytes);
    if group > 0 {
        format!("Group {group}")
    } else {
        String::new()
    }
}

fn json_str(out: &mut String, s: &str) {
    let mut escaped = Vec::new();
    crate::escape::json_encode(s.as_bytes(), &mut escaped);
    out.push('"');
    out.push_str(&String::from_utf8_lossy(&escaped));
    out.push('"');
}

/// `backbones(host)` RPC: the backbone names for the Settings dropdown and what
/// is known about `host` (empty for none).
pub fn rpc_json(host: &str) -> String {
    let catalog = catalog();
    let mut out = String::from("{\"Backbones\":[");
    for (i, b) in catalog.backbones.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"Name\":");
        json_str(&mut out, &b.name);
        out.push_str(",\"Providers\":[");
        for (j, p) in b.providers.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            json_str(&mut out, p);
        }
        out.push_str("]}");
    }
    let found = catalog.lookup(host);
    out.push_str("],\"Host\":");
    json_str(&mut out, host.trim());
    let _ = write!(out, ",\"Match\":\"{}\",\"Provider\":", found.kind.as_str());
    match &found.provider {
        Some(p) => json_str(&mut out, p),
        None => out.push_str("null"),
    }
    out.push_str(",\"Matches\":[");
    for (i, b) in found.backbones.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json_str(&mut out, b);
    }
    out.push_str("]}");
    out
}

pub fn rpc_reply(is_json: bool, host: &[u8]) -> Vec<u8> {
    let json = rpc_json(&String::from_utf8_lossy(host));
    if is_json {
        json.into_bytes()
    } else {
        crate::sysmetrics::to_xml(&json).into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_list_parses_and_is_consistent() {
        let c = parse(DATA).expect("data/backbones.yaml parses");
        assert_eq!(c.backbones.len(), 10);
        assert_eq!(c.backbones[0].name, "Omicron");
        assert!(c.backbones[0].providers.contains(&"iLoad USENET".to_string()));
        // every provider has a domain, and every domain names a known provider
        for b in &c.backbones {
            for p in &b.providers {
                assert!(
                    c.domains.iter().any(|(dp, _)| normalize(dp) == normalize(p)),
                    "no domain for provider {p}"
                );
            }
        }
        for (p, d) in &c.domains {
            assert!(!c.backbones_of(p).is_empty(), "domain {d} for unknown provider {p}");
            assert_eq!(
                c.domains.iter().filter(|(_, d2)| d2 == d).count(),
                1,
                "domain {d} listed twice"
            );
        }
        // every confirmed host names a known backbone
        for (h, b) in &c.hosts {
            assert!(
                c.backbones.iter().any(|x| normalize(&x.name) == normalize(b)),
                "host {h} names unknown backbone {b}"
            );
        }
    }

    #[test]
    fn lookup_confirmed_suggested_unknown_skipped() {
        let c = parse(DATA).unwrap();
        let l = c.lookup("News.Eweka.NL.");
        assert_eq!(l.kind, MatchKind::Confirmed);
        assert_eq!(l.backbones, vec!["Eweka Internet Services"]);
        assert_eq!(l.provider.as_deref(), Some("Eweka"));

        // a single-backbone provider's domain is still only a suggestion
        let l = c.lookup("eu.xlned.com");
        assert_eq!(l.kind, MatchKind::Suggested);
        assert_eq!(l.backbones, vec!["Omicron"]);

        // a reseller of several backbones: all of them, in list order
        let l = c.lookup("news.newsgroupdirect.com");
        assert_eq!(l.kind, MatchKind::Suggested);
        assert_eq!(l.provider.as_deref(), Some("NewsgroupDirect"));
        assert_eq!(
            l.backbones,
            vec!["UsenetExpress", "Giganews", "Its Hosted", "Uzo Reto"]
        );

        // suffix match on a dot boundary only
        assert_eq!(c.lookup("notxlned.com").kind, MatchKind::Unknown);
        assert_eq!(c.lookup("news.usenet.farm").provider.as_deref(), Some("Usenet.Farm"));
        assert_eq!(c.lookup("news.example.org").kind, MatchKind::Unknown);

        for skipped in ["127.0.0.1", "::1", "[2001:db8::1]", "localhost", "proxy", ""] {
            assert_eq!(c.lookup(skipped).kind, MatchKind::Skipped, "{skipped}");
        }
    }

    #[test]
    fn names_compare_loosely_and_intern_stably() {
        assert_eq!(normalize("  Its   Hosted "), "its hosted");
        assert_eq!(display_name("its hosted"), "Its Hosted");
        assert_eq!(display_name("  My  Own  "), "My Own");
        let a = intern("Uzo Reto");
        assert!(a > 0);
        assert_eq!(intern("uzo  reto"), a);
        assert_ne!(intern("Test Backbone X"), a);
        assert_eq!(intern("   "), 0);
        assert_eq!(name_of(a).as_deref(), Some("Uzo Reto"));
        assert_eq!(name_of(0), None);
    }

    #[test]
    fn legacy_group_becomes_a_custom_backbone() {
        assert_eq!(server_backbone(None, Some(b"3")), "Group 3");
        assert_eq!(server_backbone(None, Some(b"0")), "");
        assert_eq!(server_backbone(None, None), "");
        assert_eq!(server_backbone(Some(b"omicron"), Some(b"3")), "Omicron");
        assert_eq!(server_backbone(Some(b""), Some(b"3")), "Group 3");
        assert_eq!(server_backbone(Some(b"  "), None), "");
    }

    #[test]
    fn rpc_lists_backbones_and_the_host_match() {
        let j = rpc_json("news.eweka.nl");
        assert!(j.starts_with("{\"Backbones\":[{\"Name\":\"Omicron\",\"Providers\":[\"AnonyNews\""));
        assert!(j.contains("\"Match\":\"confirmed\",\"Provider\":\"Eweka\",\"Matches\":[\"Eweka Internet Services\"]"));
        let j = rpc_json("");
        assert!(j.ends_with("\"Host\":\"\",\"Match\":\"skipped\",\"Provider\":null,\"Matches\":[]}"));
    }
}
