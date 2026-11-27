//! GDK runtime coverage read from the shared inventory, like
//! `emu/translation.py` (`gdk_runtime_area`) and `emu/status_map.py` (`gdk_blocks`) do:
//! `gdk_api_table.inc` lists every X* function, `served_interfaces.inc` the interfaces AnyXboxX
//! serves, and `unsupported.txt` lists refused methods, including explicit uninstall refusals.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

static API_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"ANYXBOXX_GDK_API\((\w+), "([^"]+)", "([^"]+)", (\d+)\)"#).expect("pattern")
});
static SERVED_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"ANYXBOXX_SERVED_INTERFACE\(\w+, "([^"]+)", "([^"]+)"\)"#).expect("pattern")
});
static NOT_IMPLEMENTED_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"ANYXBOXX_NOT_IMPLEMENTED,\s*//\s*\d+\s+(X\w+)").expect("pattern")
});
static FIRST_WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^X[A-Z][a-z0-9]*").expect("pattern"));

/// One X* function: the GUIDs of its provider and interface (`tools/gdk_interfaces.GdkApi`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GdkApi {
    pub name: String,
    pub provider: String,
    pub interface: String,
}

/// The three inputs, by default `runtime/xgameruntime` of the repository.
#[derive(Debug, Clone)]
pub struct GdkSources {
    pub table: PathBuf,
    pub served: PathBuf,
    pub sources: PathBuf,
}

impl GdkSources {
    pub fn in_folder(folder: &Path) -> Self {
        Self {
            table: folder.join("gdk_api_table.inc"),
            served: folder.join("served_interfaces.inc"),
            sources: folder.to_path_buf(),
        }
    }
}

/// What the sources say, read once.
#[derive(Debug, Clone)]
pub struct GdkRuntime {
    pub apis: Vec<GdkApi>,
    /// Served (provider, interface), upper case.
    pub served: BTreeSet<(String, String)>,
    /// Functions wired to the stub.
    pub stubs: BTreeSet<String>,
}

pub fn read(sources: &GdkSources) -> std::io::Result<GdkRuntime> {
    let table = std::fs::read_to_string(&sources.table)?;
    let apis = API_LINE
        .captures_iter(&table)
        .map(|c| GdkApi {
            name: c[1].to_string(),
            provider: c[2].to_string(),
            interface: c[3].to_string(),
        })
        .collect();
    let stubs = not_implemented_functions(&sources.sources)?;
    let served = SERVED_LINE
        .captures_iter(&std::fs::read_to_string(&sources.served)?)
        .map(|c| (c[1].to_uppercase(), c[2].to_uppercase()))
        .collect();
    Ok(GdkRuntime {
        apis,
        served,
        stubs,
    })
}

/// Current runtime refusals, or the C++ table comments for an older repository.
fn not_implemented_functions(folder: &Path) -> std::io::Result<BTreeSet<String>> {
    match std::fs::read_to_string(folder.join("unsupported.txt")) {
        Ok(text) => return Ok(text.lines().map(String::from).collect()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut names = BTreeSet::new();
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Ok(names); // Path.glob on a missing folder finds nothing
    };
    for path in entries.flatten().map(|e| e.path()) {
        let cpp = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("cpp"));
        if cpp {
            let text = std::fs::read_to_string(&path)?;
            names.extend(
                NOT_IMPLEMENTED_LINE
                    .captures_iter(&text)
                    .map(|c| c[1].to_string()),
            );
        }
    }
    Ok(names)
}

impl GdkRuntime {
    fn key(api: &GdkApi) -> (String, String) {
        (api.provider.clone(), api.interface.clone())
    }

    /// Written by AnyXboxX: its interface is served and it is not a stub.
    pub fn is_done(&self, api: &GdkApi) -> bool {
        self.served.contains(&Self::key(api)) && !self.stubs.contains(&api.name)
    }

    pub fn done(&self) -> usize {
        self.apis.iter().filter(|a| self.is_done(a)).count()
    }

    /// (served interfaces the table knows, interfaces of the table).
    pub fn interfaces(&self) -> (usize, usize) {
        let all: BTreeSet<(String, String)> = self.apis.iter().map(Self::key).collect();
        (all.intersection(&self.served).count(), all.len())
    }

    /// `status_map.gdk_blocks`: (interface name, done, total), sorted by name; equal names keep
    /// the table's order.
    pub fn blocks(&self) -> Vec<(String, usize, usize)> {
        let names = groups(&self.apis);
        let mut order: Vec<(String, String)> = Vec::new();
        let mut counts: HashMap<(String, String), (usize, usize)> = HashMap::new();
        for api in &self.apis {
            let key = Self::key(api);
            let entry = counts.entry(key.clone()).or_insert_with(|| {
                order.push(key);
                (0, 0)
            });
            entry.0 += usize::from(self.is_done(api));
            entry.1 += 1;
        }
        order.sort_by(|a, b| names[a].cmp(&names[b]));
        order
            .into_iter()
            .map(|key| {
                let (done, total) = counts[&key];
                (names[&key].clone(), done, total)
            })
            .collect()
    }
}

/// `tools/gdk_interfaces.groups`: an interface is named by the common prefix of its functions
/// that start with its most frequent first word (ties: the first word met).
pub fn groups(apis: &[GdkApi]) -> HashMap<(String, String), String> {
    let mut order: Vec<(String, String)> = Vec::new();
    let mut by_key: HashMap<(String, String), Vec<&str>> = HashMap::new();
    for api in apis {
        let key = GdkRuntime::key(api);
        by_key
            .entry(key.clone())
            .or_insert_with(|| {
                order.push(key);
                Vec::new()
            })
            .push(&api.name);
    }
    order
        .into_iter()
        .map(|key| {
            let name = group_name(&by_key[&key]);
            (key, name)
        })
        .collect()
}

fn group_name(functions: &[&str]) -> String {
    let mut words: Vec<(&str, usize)> = Vec::new();
    for function in functions {
        // Python raises on a name without a first word; such a name cannot come from the table.
        let word = FIRST_WORD.find(function).map_or("", |m| m.as_str());
        match words.iter_mut().find(|(w, _)| *w == word) {
            Some((_, count)) => *count += 1,
            None => words.push((word, 1)),
        }
    }
    // Counter.most_common(1): the highest count, the first one met on a tie.
    let best = words.iter().map(|(_, c)| *c).max().unwrap_or(0);
    let word = words
        .iter()
        .find(|(_, c)| *c == best)
        .map_or("", |(w, _)| *w);
    let family: Vec<&str> = functions
        .iter()
        .copied()
        .filter(|f| f.starts_with(word))
        .collect();
    let prefix = common_prefix(&family);
    let name = prefix.trim_end_matches('_');
    if name.is_empty() { word } else { name }.to_string()
}

/// `os.path.commonprefix`, character by character.
fn common_prefix(names: &[&str]) -> String {
    let Some(first) = names.first() else {
        return String::new();
    };
    let mut prefix: Vec<char> = first.chars().collect();
    for name in &names[1..] {
        let shared = prefix
            .iter()
            .zip(name.chars())
            .take_while(|(a, b)| *a == b)
            .count();
        prefix.truncate(shared);
    }
    prefix.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api(name: &str, interface: &str) -> GdkApi {
        GdkApi {
            name: name.into(),
            provider: "P".into(),
            interface: interface.into(),
        }
    }

    #[test]
    fn groups_take_the_prefix_of_the_main_word() {
        let apis = [
            api("XUserAddAsync", "A"),
            api("XUserGetId", "A"),
            api("XGameUiShow", "A"),
            api("XTaskQueueCreate", "B"),
            api("XTaskQueue_Close", "B"),
        ];
        let names = groups(&apis);
        assert_eq!(names[&("P".into(), "A".into())], "XUser");
        assert_eq!(names[&("P".into(), "B".into())], "XTaskQueue");
    }

    #[test]
    fn a_tie_keeps_the_first_word() {
        let names = groups(&[api("XBetaOne", "A"), api("XAlphaTwo", "A")]);
        assert_eq!(names[&("P".into(), "A".into())], "XBetaOne");
    }

    #[test]
    fn the_stub_comment_may_start_on_the_next_line() {
        let text =
            "ANYXBOXX_NOT_IMPLEMENTED,\n    // 12 XStoreQuery\nANYXBOXX_NOT_IMPLEMENTED, // x XNo";
        let found: Vec<&str> = NOT_IMPLEMENTED_LINE
            .captures_iter(text)
            .map(|c| c.get(1).unwrap().as_str())
            .collect();
        assert_eq!(found, ["XStoreQuery"]);
    }

    #[test]
    fn the_table_header_comment_is_not_an_api() {
        let text = "// ANYXBOXX_GDK_API(fonction, fournisseur, interface, n° de méthode)\n\
                    ANYXBOXX_GDK_API(XA, \"p\", \"i\", 3)";
        assert_eq!(API_LINE.captures_iter(text).count(), 1);
    }
}
