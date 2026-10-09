// GUARD: conventions naming and shape rules over one item pass.

use crate::guards::{
    lexer::{File, IMPL, Item, TEST, WIDGET, is_word},
    support,
};

type Hits = Vec<String>;

const BANNED: &str = "outcome apply look adjust failure rejected perform handle \
                      process dispatch supervise";
const LOOPS: &[&str] = &["MainLoop", "DriverLoop", "EventLoop", "AbLoop"];

fn key_if(hit: bool, key: &str) -> Hits {
    hit.then(|| key.to_owned()).into_iter().collect()
}

fn camel_words(name: &str) -> Vec<&str> {
    let mut cuts: Vec<usize> = name
        .char_indices()
        .filter(|(at, c)| *at == 0 || c.is_uppercase())
        .map(|(at, _)| at)
        .collect();
    cuts.push(name.len());
    cuts.array_windows()
        .map(|&[start, end]| &name[start..end])
        .collect()
}

fn banned(on: bool, words: Vec<&str>, name: &str) -> Hits {
    let any = words
        .iter()
        .any(|word| BANNED.split(' ').any(|ban| ban == word.to_lowercase()));
    key_if(on && any, name)
}

fn type_words(_: &File, site: &Item) -> Hits {
    banned(site.is_type(), camel_words(&site.name), &site.name)
}

fn fn_words(_: &File, site: &Item) -> Hits {
    let on = site.kind == "fn" && !site.has(TEST);
    banned(on, site.name.split('_').collect(), &site.name)
}

fn machine_state(_: &File, site: &Item) -> Hits {
    let hit = site.kind == "impl" && site.via == "Machine";
    key_if(hit && site.name.ends_with("State"), &site.name)
}

fn loop_types(_: &File, site: &Item) -> Hits {
    let name = site.name.as_str();
    key_if(
        site.is_type() && name.ends_with("Loop") && !LOOPS.contains(&name),
        name,
    )
}

fn colors(_: &File, site: &Item) -> Hits {
    let name = site.name.as_str();
    let palette = name.ends_with("Colors") && name != "Colors";
    key_if(site.is_type() && palette && !name.starts_with("Toml"), name)
}

fn toml_shape(file: &File, site: &Item) -> Hits {
    let name = site.name.as_str();
    let shaped = matches!(site.kind.as_str(), "struct" | "enum")
        && (name.ends_with("File") || name.ends_with("Config"))
        && !name.starts_with("Toml");
    let serde = (site.head..site.at)
        .any(|at| matches!(file.tx(at), "Deserialize" | "Serialize"));
    key_if(file.path.starts_with("config/") && shaped && serde, name)
}

fn request(file: &File, site: &Item) -> Hits {
    let hit = site.is_type() && site.name.ends_with("Request");
    key_if(hit && !file.path.starts_with("kernel/"), &site.name)
}

fn heads(file: &File, from: usize, to: usize) -> Vec<usize> {
    let (mut depth, mut start, mut out) = (0_i32, true, Vec::new());
    for at in from..to {
        match file.tx(at) {
            "(" | "[" | "{" | "<" => depth += 1,
            ")" | "]" | "}" => depth -= 1,
            ">" if file.tx(at - 1) != "-" => depth -= 1,
            "," if depth == 0 => start = true,
            word if depth == 0 && start && is_word(word) => {
                start = false;
                out.push(at);
            }
            _ => {}
        }
    }
    out
}

fn single_named(file: &File, enum_name: &str, variant: usize) -> Hits {
    let close = file.matching_close(variant + 1);
    let fields = heads(file, variant + 2, close);
    let named: Vec<&str> = fields
        .iter()
        .filter(|at| file.tx(*at + 1) == ":")
        .map(|at| file.tx(*at))
        .collect();
    let single = file.tx(variant + 1) == "{"
        && matches!(named.as_slice(), [field] if !["rows", "visible_rows"].contains(field));
    key_if(single, &format!("{enum_name}::{}", file.tx(variant)))
}

fn single_field(file: &File, site: &Item) -> Hits {
    let open = (site.at..file.tokens.len()).find(|at| file.tx(*at) == "{");
    let plain = site.kind == "enum" && !site.name.ends_with("Error");
    let Some(open) = open.filter(|_| plain) else {
        return Hits::new();
    };
    let variants = heads(file, open + 1, file.matching_close(open));
    let single = |variant| single_named(file, &site.name, variant);
    variants.into_iter().flat_map(single).collect()
}

fn aliases_result(file: &File, name_at: usize) -> bool {
    let (mut at, mut depth) = (name_at + 1, 0_i32);
    while !file.tx(at).is_empty() && (depth > 0 || file.tx(at) == "<") {
        depth += i32::from(file.tx(at) == "<") - i32::from(file.tx(at) == ">");
        at += 1;
    }
    let equals = file.tx(at) == "=";
    at += 1;
    while file.tx(at + 1) == ":" && file.tx(at + 2) == ":" {
        at += 3;
    }
    equals && file.tx(at) == "Result"
}

fn result_alias(file: &File, site: &Item) -> Hits {
    let hit = site.kind == "type" && !site.has(IMPL);
    key_if(hit && aliases_result(file, site.at + 1), &site.name)
}

fn render(_: &File, site: &Item) -> Hits {
    let hit = site.kind == "fn" && site.name.starts_with("render");
    key_if(hit && !site.has(WIDGET) && !site.has(TEST), &site.name)
}

fn mem_take(_: &File, site: &Item) -> Hits {
    key_if(
        site.kind == "take",
        &format!("mem::take(self.{})", site.name),
    )
}

type Rule = (&'static str, &'static str, fn(&File, &Item) -> Hits);

const RULES: &[Rule] = &[
    (
        "type_words",
        "retired word in a type name (§10)",
        type_words,
    ),
    (
        "fn_words",
        "retired word in a fn name (§4.3, §10)",
        fn_words,
    ),
    (
        "machine_state",
        "a part has no `State` suffix (§3.7)",
        machine_state,
    ),
    (
        "loop",
        "only MainLoop, DriverLoop, EventLoop (§10)",
        loop_types,
    ),
    ("colors", "only the theme palette is `Colors` (§2)", colors),
    (
        "toml_shape",
        "a serde shape is named `Toml*` (§2)",
        toml_shape,
    ),
    (
        "request",
        "`*Request` types live in the kernel (§2)",
        request,
    ),
    (
        "single_field",
        "one field means a tuple variant (§5.1)",
        single_field,
    ),
    (
        "result_alias",
        "write `Result<..>` out, no alias (§5.5)",
        result_alias,
    ),
    ("render", "`fn render*` only in impl Widget (§10)", render),
    (
        "mem_take",
        "`transition` uses `mem::replace` (§3.5)",
        mem_take,
    ),
];

fn discards(line: &str) -> bool {
    let rest = line.trim_start().strip_prefix("let _");
    rest.is_some_and(|tail| tail.starts_with([' ', ':', '=']))
}

fn discard_hits() -> Vec<(String, usize, String)> {
    let hit = |(relative, path): &(String, std::path::PathBuf)| {
        let lines: Vec<String> =
            support::read(path).lines().map(str::to_owned).collect();
        let at = lines.iter().enumerate().filter(|(_, line)| discards(line));
        at.map(|(index, _)| (relative.clone(), index + 1, "discard".to_owned()))
            .collect::<Vec<_>>()
    };
    support::source_files(&["src", "tests"])
        .iter()
        .flat_map(hit)
        .collect()
}

fn bracket(text: &str) -> i32 {
    match text {
        "(" | "[" | "{" => 1,
        ")" | "]" | "}" => -1,
        _ => 0,
    }
}

fn statement_start(file: &File, dot: usize) -> usize {
    let mut depth = 0_i32;
    let start = (0..dot).rev().find(|at| {
        let text = file.tx(*at);
        depth -= bracket(text);
        depth < 0 || (depth == 0 && text == ";")
    });
    start.map_or(0, |at| at + 1)
}

fn assignment(file: &File, at: usize) -> bool {
    file.tx(at) == "="
        && !matches!(file.tx(at.wrapping_sub(1)), "=" | "!" | "<" | ">")
        && !matches!(file.tx(at + 1), "=" | ">")
}

fn binds(file: &File, from: usize, to: usize) -> bool {
    let mut depth = 0_i32;
    (from..to).any(|at| {
        depth += bracket(file.tx(at));
        let keyword = matches!(file.tx(at), "let" | "return" | "break");
        depth == 0 && (keyword || assignment(file, at))
    })
}

fn exempt_spans(file: &File) -> Vec<(usize, usize)> {
    let drops = file
        .items
        .iter()
        .filter(|site| site.kind == "impl" && site.via == "Drop")
        .map(|site| site.at);
    let hooks = (0..file.tokens.len()).filter(|at| file.tx(*at) == "set_hook");
    let span = |at: usize| {
        let open =
            (at..file.tokens.len()).find(|x| matches!(file.tx(*x), "{" | "("))?;
        Some((open, file.matching_close(open)))
    };
    drops.chain(hooks).filter_map(span).collect()
}

fn ok_discards(file: &File) -> Vec<usize> {
    let exempt = exempt_spans(file);
    let shape = [".", "ok", "(", ")", ";"];
    let at_shape = |at: &usize| {
        let mut texts = shape.iter().enumerate();
        texts.all(|(offset, text)| file.tx(at + offset) == *text)
    };
    (0..file.tokens.len())
        .filter(at_shape)
        .filter(|at| !binds(file, statement_start(file, *at), *at))
        .filter(|at| !exempt.iter().any(|(open, close)| open < at && at < close))
        .map(|at| file.tokens[at].line)
        .collect()
}

fn ok_hits(files: &[File]) -> Vec<(String, usize, String)> {
    let hits = |file: &File| {
        let lines = ok_discards(file).into_iter();
        lines
            .map(|line| (file.path.clone(), line, ".ok();".to_owned()))
            .collect::<Vec<_>>()
    };
    files.iter().flat_map(hits).collect()
}

fn in_src(file: &File) -> bool {
    file.path.split('/').nth(1) == Some("src")
}

const IN_TESTS_TOO: &[&str] = &["result_alias"];

fn item_hits(files: &[File], rule: &Rule) -> Vec<(String, usize, String)> {
    let (name, _, find) = rule;
    let sites = files
        .iter()
        .filter(|file| in_src(file) || IN_TESTS_TOO.contains(name))
        .flat_map(|file| file.items.iter().map(move |site| (file, site)));
    sites
        .flat_map(|(file, site)| {
            let hit = |key| (file.path.clone(), site.line, key);
            find(file, site).into_iter().map(hit)
        })
        .collect()
}

type Check = (&'static str, &'static str, Vec<(String, usize, String)>);

fn checks(files: &[File]) -> Vec<Check> {
    let mut checks: Vec<Check> = RULES
        .iter()
        .map(|rule| (rule.0, rule.1, item_hits(files, rule)))
        .collect();
    let discard =
        "write `.unwrap()` in tests, handle the value elsewhere, never `let _` (§6.3)";
    checks.push(("let_underscore", discard, discard_hits()));
    let ok = "handle the `Result`, never drop it with `.ok();` (§6.3)";
    checks.push(("ok_discard", ok, ok_hits(files)));
    checks
}

#[test]
fn conventions_hold() {
    let files: Vec<File> = support::source_files(&["src", "tests"])
        .iter()
        .map(|(relative, path)| File::parse(relative, &support::read(path)))
        .collect();
    let violations: Vec<String> = checks(&files)
        .into_iter()
        .flat_map(|(rule, message, hits)| {
            hits.into_iter().map(move |(path, line, key)| {
                format!("{path}:{line}: `{key}` — {message} [{rule}]")
            })
        })
        .collect();
    support::report("conventions guard (docs/conventions.md):", &violations);
}

#[test]
fn discard_is_only_a_let_underscore_binding() {
    let hits = [
        "let _ = x;",
        "  let _: u8 = x;",
        "let _guard = x;",
        "let __ = x;",
    ];
    assert_eq!(hits.map(discards), [true, true, false, false]);
}

const OK_SAMPLE: &str = r"fn f() {
    a.ok();
    let b = c.ok();
    d = e.ok();
    if x == y { g.ok(); }
    return h.ok();
}
impl Drop for S { fn drop(&mut self) { i.ok(); } }
fn hook() { std::panic::set_hook(Box::new(|_| { j.ok(); })); }
";

#[test]
fn ok_discard_is_a_dropped_statement_outside_drop_and_the_hook() {
    let file = File::parse("kernel/src/sample.rs", OK_SAMPLE);
    assert_eq!(ok_discards(&file), [2, 5]);
}

const SAMPLE: &str = r##"
pub enum Event { Seek { to: u8 }, After { delay: u8, timer: u8 }, Play }
pub enum LoadError { Device { requested: String } }
pub enum Scroll { By { rows: i32 } }
struct PlayOutcome;
struct PlayLoop;
struct SkinColors;
#[derive(Serialize)]
struct AudioConfig;
struct SeekRequest;
type Alias = std::result::Result<u8, String>;
struct PanelState;
impl Machine for PanelState {
    fn transition(&mut self, message: u8) {
        let old = std::mem::take(&mut self.rows);
        let ok = mem::replace(&mut self.rows, 0);
        let text = r#"fn render_fake { take(&mut self.x) }"#;
    }
    fn apply_it(&self) {}
}
impl Widget for PanelState {
    fn render(self, area: u8) {}
}
impl PanelState {
    fn render_in(&self) {}
}
"##;

#[test]
fn every_rule_sees_what_it_is_meant_to() {
    let file = File::parse("config/src/sample.rs", SAMPLE);
    let mut seen = Vec::new();
    for (rule, _, find) in RULES {
        for key in file.items.iter().flat_map(|site| find(&file, site)) {
            seen.push(format!("{rule}:{key}"));
        }
    }
    let expected = "type_words:PlayOutcome fn_words:apply_it machine_state:PanelState \
        loop:PlayLoop colors:SkinColors toml_shape:AudioConfig request:SeekRequest \
        single_field:Event::Seek result_alias:Alias render:render_in \
        mem_take:mem::take(self.rows)";
    assert_eq!(seen.join(" "), expected);
}
