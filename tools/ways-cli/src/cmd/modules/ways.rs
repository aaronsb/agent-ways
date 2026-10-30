//! A way as an item: found in the three roots of ADR-143, the shipped
//! corpus, the user's own and the project's. The lowest root that holds a
//! way's id is its origin, and a way of the same id in a higher root
//! replaces it, as `ways corpus` and the hooks read them.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use agent_settings::item::{Entry, Family, How, Item, Origin, Override, WayState};
use serde::Serialize;

/// One root's copy of a way.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub origin: Origin,
    /// The root, which a way's file is named against.
    pub root: PathBuf,
    /// None when the directory holds no way file sessions would read.
    pub file: Option<PathBuf>,
}

/// Every way id in the roots, with each root that holds it, lowest first:
/// the shipped `corpus`, the `user`'s root and the `project`'s, when it has
/// one.
pub fn found(corpus: &Path, user: &Path, project: Option<&Path>) -> BTreeMap<String, Vec<Found>> {
    let mut out: BTreeMap<String, Vec<Found>> = BTreeMap::new();
    let roots = [(Origin::Builtin, Some(corpus)), (Origin::User, Some(user)), (Origin::Project, project)];
    for (origin, root) in roots {
        let Some(root) = root else { continue };
        for id in crate::cmd::scan::candidates::way_ids(root) {
            let file = crate::session::find_way_in_dir(&root.join(&id));
            out.entry(id).or_default().push(Found { origin, root: root.to_path_buf(), file });
        }
    }
    out
}

/// A way has no fields of its own beyond the descriptor.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WayFields {}

/// The way `id`, from the roots that hold it (`found`, lowest first), in
/// the state `cfg` gives it: `disabled` by a switch or its domain,
/// `invalid` when the file that runs is missing or its frontmatter does
/// not parse.
pub fn item(id: &str, found: &[Found], cfg: &crate::config::Config) -> Entry<WayFields> {
    let base = found.first();
    let origin = base.map_or(Origin::Builtin, |f| f.origin);
    let overrides = found.iter().skip(1).filter_map(|f| Some(Override { origin: f.origin, how: How::Replaces, path: f.file.clone()? })).collect();
    let mut item = Item { path: base.and_then(|f| f.file.clone()), ..Item::new(Family::Way, id, origin) }.with_overrides(overrides);
    let domain = id.split('/').next().unwrap_or(id);
    let broken = match found.last().map(|f| f.file.as_deref()) {
        Some(Some(file)) => crate::frontmatter::parse(file).err().map(|e| format!("{e:#}")),
        _ => Some("no way file in its directory, so sessions skip it".into()),
    };
    let (state, detail) = if let Some(why) = broken {
        (WayState::Invalid, Some(why))
    } else if cfg.disabled_domains.iter().any(|d| d == domain) {
        (WayState::Disabled, Some(format!("its domain {domain} is in disabled_domains")))
    } else if let Some(key) = cfg.disabling_toggle(id) {
        (WayState::Disabled, Some(format!("switched off by the {key} switch")))
    } else {
        (WayState::Enabled, None)
    };
    item.detail = detail;
    Entry { item: item.with_state(state), fields: WayFields {} }
}

/// Every way a session in `project` can fire, by id, each in the state the
/// project's config gives it.
pub fn all(corpus: &Path, user: &Path, project: &Path) -> Vec<Entry<WayFields>> {
    let cfg = crate::config::Config::load(&project.to_string_lossy());
    found(corpus, user, crate::cmd::ways_roots::project_ways(project).as_deref()).iter().map(|(id, f)| item(id, f, &cfg)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn way(root: &Path, id: &str, front: &str) {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{}.md", id.rsplit('/').next().unwrap())), format!("---\n{front}\n---\n# body\n")).unwrap();
    }

    #[test]
    fn a_way_takes_its_lowest_root_as_origin_and_a_higher_one_replaces_it() {
        let d = std::env::temp_dir().join(format!("ways-items-ways-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (corpus, user, project) = (d.join("corpus"), d.join("user"), d.join("proj"));
        way(&corpus, "softwaredev/code", "description: shipped\nvocabulary: code");
        way(&user, "softwaredev/code", "description: mine\nvocabulary: code");
        way(&project, "softwaredev/code", "description: the project's\nvocabulary: code");
        way(&project, "local/build", "description: build\nvocabulary: build");
        way(&user, "mine/broken", "description: a: b: c");
        way(&corpus, "ea/mail", "description: mail\nvocabulary: mail");
        let found = found(&corpus, &user, Some(&project));
        let mut cfg = crate::config::Config::default();
        cfg.disabled_domains = vec!["ea".into()];
        let get = |id: &str| item(id, &found[id], &cfg).item;

        let code = get("softwaredev/code");
        assert_eq!((code.family, code.origin, code.kind), (Family::Way, Origin::Builtin, None));
        assert_eq!(code.path.as_deref(), Some(corpus.join("softwaredev/code/code.md").as_path()));
        let how: Vec<_> = code.overrides.iter().map(|o| (o.origin, o.how, o.path.clone())).collect();
        assert_eq!(how, [(Origin::User, How::Replaces, user.join("softwaredev/code/code.md")), (Origin::Project, How::Replaces, project.join("softwaredev/code/code.md"))]);
        assert_eq!(code.state, WayState::Enabled.into());

        let build = get("local/build");
        assert_eq!((build.origin, build.overrides.len()), (Origin::Project, 0));
        let broken = get("mine/broken");
        assert_eq!((broken.origin, broken.state), (Origin::User, WayState::Invalid.into()));
        assert!(broken.detail.is_some());
        let mail = get("ea/mail");
        assert_eq!((mail.state, mail.detail.as_deref()), (WayState::Disabled.into(), Some("its domain ea is in disabled_domains")));
        let _ = std::fs::remove_dir_all(&d);
    }
}
