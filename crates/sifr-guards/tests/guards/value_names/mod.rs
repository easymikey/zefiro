use std::collections::{BTreeMap, BTreeSet};

use proc_macro2::Ident;
use syn::{
    Expr,
    ExprClosure,
    Fields,
    FnArg,
    GenericArgument,
    Generics,
    ImplItemFn,
    ItemEnum,
    ItemFn,
    ItemImpl,
    ItemStruct,
    ItemTrait,
    ItemType,
    ItemUnion,
    Local,
    Pat,
    PathArguments,
    PathSegment,
    Signature,
    TraitItemFn,
    Type,
    TypePath,
    visit::{self, Visit},
};

use crate::guards::support;

mod tables;

use tables::{
    COLLECTION_ROLES,
    COLLECTIONS,
    CONSTRUCTOR_PREFIXES,
    CONSTRUCTORS,
    ERROR_WORDS,
    EXTERNAL_CHECKED,
    PAIR_WORDS,
    RESERVED,
    ROLE_NAMED,
    ROLE_WORDS,
    WRAPPERS,
    ends_in,
    listed,
    reserved_clash,
};

const BASELINE: &str = include_str!("../value_names_baseline.txt");

#[derive(Clone, Copy)]
enum Kind {
    Field,
    Param,
    Let,
    Reserved,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Self::Field => "field",
            Self::Param => "param",
            Self::Let => "let",
            Self::Reserved => "reserved",
        }
    }
}

#[derive(Clone, Copy)]
enum ParamScope {
    Checked,
    ExternalTrait,
}

struct Finding {
    path: String,
    line: usize,
    kind: Kind,
    name: String,
    display: String,
    word: String,
}

impl Finding {
    fn key(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}",
            self.path,
            self.kind.label(),
            self.name,
            self.display,
            self.word
        )
    }

    fn describe(&self) -> String {
        if let Kind::Reserved = self.kind {
            return format!(
                "{}:{}: `{}` has type `{}`; `{}` is the domain word of `{}`\n\
                 BASELINE\t{}",
                self.path,
                self.line,
                self.name,
                self.display,
                self.word,
                listed(RESERVED, &self.word)
                    .collect::<Vec<_>>()
                    .join("`, `"),
                self.key()
            );
        }
        format!(
            "{}:{}: `{}` is a {} of type `{}`; its name must be or end with `{}` \
             (or its trailing words, a role word, or the type word without the owner's words)\n\
             BASELINE\t{}",
            self.path,
            self.line,
            self.name,
            self.kind.label(),
            self.display,
            self.word,
            self.key()
        )
    }
}

#[derive(Default)]
struct Catalog {
    types: BTreeSet<String>,
    enums: BTreeSet<String>,
    traits: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for Catalog {
    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        self.types.insert(node.ident.to_string());
        visit::visit_item_struct(self, node);
    }

    fn visit_item_enum(&mut self, node: &'ast ItemEnum) {
        self.types.insert(node.ident.to_string());
        self.enums.insert(node.ident.to_string());
        visit::visit_item_enum(self, node);
    }

    fn visit_item_union(&mut self, node: &'ast ItemUnion) {
        self.types.insert(node.ident.to_string());
        visit::visit_item_union(self, node);
    }

    fn visit_item_type(&mut self, node: &'ast ItemType) {
        self.types.insert(node.ident.to_string());
        visit::visit_item_type(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast ItemTrait) {
        self.traits.insert(node.ident.to_string());
        visit::visit_item_trait(self, node);
    }
}

struct Mentions<'a> {
    catalog: &'a Catalog,
    words: BTreeSet<String>,
}

impl Mentions<'_> {
    fn note(&mut self, ident: &Ident) {
        let name = ident.to_string();
        if self.catalog.types.contains(&name) {
            let word = snake(&name);
            self.words.insert(plural(&word));
            self.words.insert(word);
        }
    }
}

impl<'ast> Visit<'ast> for Mentions<'_> {
    fn visit_path_segment(&mut self, node: &'ast syn::PathSegment) {
        self.note(&node.ident);
        visit::visit_path_segment(self, node);
    }

    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        self.note(&node.ident);
        visit::visit_item_struct(self, node);
    }

    fn visit_item_enum(&mut self, node: &'ast ItemEnum) {
        self.note(&node.ident);
        visit::visit_item_enum(self, node);
    }

    fn visit_item_union(&mut self, node: &'ast ItemUnion) {
        self.note(&node.ident);
        visit::visit_item_union(self, node);
    }

    fn visit_item_type(&mut self, node: &'ast ItemType) {
        self.note(&node.ident);
        visit::visit_item_type(self, node);
    }
}

struct Resolved {
    display: String,
    word: String,
    role_key: Option<String>,
    element: Option<String>,
}

struct Slot<'a> {
    kind: Kind,
    owner: &'a [String],
    ident: &'a Ident,
    ty: &'a Type,
}

struct Scan<'a> {
    catalog: &'a Catalog,
    path: &'a str,
    mentions: BTreeSet<String>,
    generics: Vec<String>,
    scope: ParamScope,
    findings: Vec<Finding>,
}

fn snake(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (index, current) in chars.iter().enumerate() {
        let before = index.checked_sub(1).and_then(|at| chars.get(at));
        let after = chars.get(index + 1);
        let word_start = current.is_uppercase()
            && before.is_some_and(|prior| {
                prior.is_lowercase()
                    || prior.is_ascii_digit()
                    || (prior.is_uppercase()
                        && after.is_some_and(|next| next.is_lowercase()))
            });
        if word_start {
            out.push('_');
        }
        out.extend(current.to_lowercase());
    }
    out
}

fn plural(word: &str) -> String {
    let consonant_y = word.strip_suffix('y').filter(|stem| {
        stem.chars()
            .next_back()
            .is_some_and(|last| !"aeiou".contains(last))
    });
    if let Some(stem) = consonant_y {
        format!("{stem}ies")
    } else if ["s", "x", "ch", "sh"].iter().any(|end| word.ends_with(end)) {
        format!("{word}es")
    } else {
        format!("{word}s")
    }
}

fn singular(word: &str) -> String {
    if let Some(stem) = word.strip_suffix("ies") {
        format!("{stem}y")
    } else if ["ses", "xes", "ches", "shes"]
        .iter()
        .any(|end| word.ends_with(end))
    {
        word.trim_end_matches("es").to_owned()
    } else if word.ends_with("ss") {
        word.to_owned()
    } else {
        word.strip_suffix('s').unwrap_or(word).to_owned()
    }
}

fn owner_words(name: &str) -> Vec<String> {
    snake(name).split('_').map(singular).collect()
}

fn without_owner_words(word: &str, owner: &[String]) -> Option<String> {
    let parts: Vec<&str> = word.split('_').collect();
    let kept = parts
        .iter()
        .position(|part| !owner.contains(&singular(part)))?;
    (kept > 0).then(|| parts[kept..].join("_"))
}

fn role_words(key: &str) -> Vec<&'static str> {
    let mut words: Vec<&'static str> = listed(ROLE_WORDS, key).collect();
    if key.ends_with("Error") {
        words.extend(ERROR_WORDS);
    }
    words
}

fn tails(word: &str, mentions: &BTreeSet<String>) -> Vec<String> {
    let parts: Vec<&str> = word.split('_').collect();
    (1..parts.len())
        .map(|start| parts[start..].join("_"))
        .filter(|tail| !mentions.contains(tail))
        .collect()
}

fn accepted(
    resolved: &Resolved,
    owner: &[String],
    mentions: &BTreeSet<String>,
) -> Vec<String> {
    let mut words = vec![resolved.word.clone()];
    words.extend(tails(&resolved.word, mentions));
    words.extend(without_owner_words(&resolved.word, owner));
    if let Some(key) = &resolved.role_key {
        words.extend(role_words(key).into_iter().map(str::to_owned));
    }
    if let Some(key) = &resolved.element {
        words.extend(listed(COLLECTION_ROLES, key).map(str::to_owned));
    }
    words.extend(PAIR_WORDS.iter().map(|word| (*word).to_owned()));
    words
}

fn matches_any(name: &str, words: &[String]) -> bool {
    words.iter().any(|word| ends_in(name, word))
}

fn first_type_argument(arguments: &PathArguments) -> Option<&Type> {
    let PathArguments::AngleBracketed(angle) = arguments else {
        return None;
    };
    angle.args.iter().find_map(|argument| {
        if let GenericArgument::Type(ty) = argument {
            Some(ty)
        } else {
            None
        }
    })
}

impl Scan<'_> {
    fn collection(&self, element: &Type) -> Option<Resolved> {
        let inner = self.resolve(element)?;
        let nested = matches!(element, Type::Path(path) if path.path.segments.last()
            .is_some_and(|last| COLLECTIONS.contains(&last.ident.to_string().as_str())));
        Some(Resolved {
            word: if nested {
                inner.word
            } else {
                plural(&inner.word)
            },
            role_key: None,
            element: inner.role_key,
            display: inner.display,
        })
    }

    fn base_name(&self, ty: &Type) -> Option<String> {
        match ty {
            Type::Reference(reference) => self.base_name(&reference.elem),
            Type::Paren(paren) => self.base_name(&paren.elem),
            Type::Group(group) => self.base_name(&group.elem),
            Type::Slice(slice) => self.base_name(&slice.elem),
            Type::Array(array) => self.base_name(&array.elem),
            Type::Path(path) if path.qself.is_none() => self.base_path(&path.path),
            _ => None,
        }
    }

    fn base_path(&self, path: &syn::Path) -> Option<String> {
        if path.segments.first()?.ident == "Self" {
            return None;
        }
        let last = path.segments.last()?;
        let name = last.ident.to_string();
        if WRAPPERS.contains(&name.as_str()) || COLLECTIONS.contains(&name.as_str()) {
            return self.base_name(first_type_argument(&last.arguments)?);
        }
        (!self.generics.contains(&name)).then_some(name)
    }

    fn check_reserved(&mut self, slot: &Slot<'_>, name: &str) {
        let Some(base) = self.base_name(slot.ty) else {
            return;
        };
        if let Some(word) = reserved_clash(self.path, name, &base) {
            self.findings.push(Finding {
                path: self.path.to_owned(),
                line: slot.ident.span().start().line,
                kind: Kind::Reserved,
                name: name.to_owned(),
                display: base,
                word: word.to_owned(),
            });
        }
    }

    fn resolve_path(&self, path: &syn::Path) -> Option<Resolved> {
        if path.segments.first()?.ident == "Self" {
            return None;
        }
        let last = path.segments.last()?;
        let name = last.ident.to_string();
        if ROLE_NAMED.contains(&name.as_str()) {
            return None;
        }
        if WRAPPERS.contains(&name.as_str()) {
            return self.resolve(first_type_argument(&last.arguments)?);
        }
        if COLLECTIONS.contains(&name.as_str()) {
            let element = first_type_argument(&last.arguments)?;
            return self.collection(element).map(|inner| Resolved {
                display: format!("{name}<{}>", inner.display),
                ..inner
            });
        }
        let workspace_type =
            self.catalog.types.contains(&name) && !self.generics.contains(&name);
        let checked = workspace_type || EXTERNAL_CHECKED.contains(&name.as_str());
        checked.then(|| Resolved {
            word: snake(&name),
            display: name.clone(),
            role_key: Some(name),
            element: None,
        })
    }

    fn resolve(&self, ty: &Type) -> Option<Resolved> {
        match ty {
            Type::Reference(reference) => self.resolve(&reference.elem),
            Type::Paren(paren) => self.resolve(&paren.elem),
            Type::Group(group) => self.resolve(&group.elem),
            Type::Slice(slice) => self.collection(&slice.elem).map(bracketed),
            Type::Array(array) => self.collection(&array.elem).map(bracketed),
            Type::Path(path) if path.qself.is_none() => self.resolve_path(&path.path),
            Type::Path(_) | Type::TraitObject(_) | Type::ImplTrait(_) => None,
            Type::BareFn(_) | Type::Tuple(_) | Type::Ptr(_) | Type::Never(_) => None,
            _ => None,
        }
    }

    fn check(&mut self, slot: Slot<'_>) {
        let raw = slot.ident.to_string();
        let name = raw.trim_start_matches("r#");
        if name.starts_with('_') {
            return;
        }
        self.check_reserved(&slot, name);
        let Some(resolved) = self.resolve(slot.ty) else {
            return;
        };
        if slot.owner.join("_") == resolved.word
            || matches_any(name, &accepted(&resolved, slot.owner, &self.mentions))
        {
            return;
        }
        self.findings.push(Finding {
            path: self.path.to_owned(),
            line: slot.ident.span().start().line,
            kind: slot.kind,
            name: name.to_owned(),
            display: resolved.display,
            word: resolved.word,
        });
    }

    fn check_fields(&mut self, owner: &[String], fields: &Fields) {
        let Fields::Named(named) = fields else {
            return;
        };
        for field in &named.named {
            if let Some(ident) = &field.ident {
                self.check(Slot {
                    kind: Kind::Field,
                    owner,
                    ident,
                    ty: &field.ty,
                });
            }
        }
    }

    fn scoped(&mut self, generics: &Generics, walk: impl FnOnce(&mut Self)) {
        let depth = self.generics.len();
        self.generics
            .extend(generics.type_params().map(|param| param.ident.to_string()));
        walk(self);
        self.generics.truncate(depth);
    }

    fn with_scope(&mut self, scope: ParamScope, walk: impl FnOnce(&mut Self)) {
        let previous = std::mem::replace(&mut self.scope, scope);
        walk(self);
        self.scope = previous;
    }
}

impl Scan<'_> {
    fn built_type(&self, expr: &Expr) -> Option<Type> {
        match expr {
            Expr::Call(call) => match &*call.func {
                Expr::Path(func) if func.qself.is_none() => {
                    self.called_type(&func.path)
                }
                _ => None,
            },
            Expr::Struct(literal) if literal.qself.is_none() => self
                .variant_type(&literal.path)
                .or_else(|| type_prefix(&literal.path, 0)),
            Expr::Path(bare) if bare.qself.is_none() => self.variant_type(&bare.path),
            _ => None,
        }
    }

    fn called_type(&self, path: &syn::Path) -> Option<Type> {
        let last = path.segments.last()?.ident.to_string();
        if constructor(&last) {
            type_prefix(path, 1)
        } else {
            self.variant_type(path)
        }
    }

    fn variant_type(&self, path: &syn::Path) -> Option<Type> {
        let last = path.segments.last()?.ident.to_string();
        if !last.starts_with(char::is_uppercase) {
            return None;
        }
        let owner_at = path.segments.len().checked_sub(2)?;
        let owner = path.segments.iter().nth(owner_at)?;
        if self.catalog.enums.contains(&owner.ident.to_string()) {
            type_prefix(path, 1)
        } else {
            None
        }
    }
}

fn constructor(name: &str) -> bool {
    CONSTRUCTORS.contains(&name)
        || CONSTRUCTOR_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

fn type_prefix(path: &syn::Path, dropped: usize) -> Option<Type> {
    let kept = path.segments.len().checked_sub(dropped)?;
    (kept > 0).then(|| {
        Type::Path(TypePath {
            qself: None,
            path: syn::Path {
                leading_colon: None,
                segments: path
                    .segments
                    .iter()
                    .take(kept)
                    .map(|segment| PathSegment::from(segment.ident.clone()))
                    .collect(),
            },
        })
    })
}

fn bracketed(inner: Resolved) -> Resolved {
    Resolved {
        display: format!("[{}]", inner.display),
        ..inner
    }
}

impl<'ast> Visit<'ast> for Scan<'_> {
    fn visit_item_struct(&mut self, node: &'ast ItemStruct) {
        let owner = owner_words(&node.ident.to_string());
        self.scoped(&node.generics, |scan| {
            scan.check_fields(&owner, &node.fields)
        });
    }

    fn visit_item_enum(&mut self, node: &'ast ItemEnum) {
        let enum_words = node.ident.to_string();
        self.scoped(&node.generics, |scan| {
            for variant in &node.variants {
                let owner = owner_words(&format!("{enum_words}{}", variant.ident));
                scan.check_fields(&owner, &variant.fields);
            }
        });
    }

    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        self.with_scope(ParamScope::Checked, |scan| {
            scan.scoped(&node.sig.generics, |inner| {
                visit::visit_item_fn(inner, node)
            });
        });
    }

    fn visit_impl_item_fn(&mut self, node: &'ast ImplItemFn) {
        self.scoped(&node.sig.generics, |scan| {
            visit::visit_impl_item_fn(scan, node)
        });
    }

    fn visit_trait_item_fn(&mut self, node: &'ast TraitItemFn) {
        self.scoped(&node.sig.generics, |scan| {
            visit::visit_trait_item_fn(scan, node)
        });
    }

    fn visit_item_trait(&mut self, node: &'ast ItemTrait) {
        self.with_scope(ParamScope::Checked, |scan| {
            scan.scoped(&node.generics, |inner| visit::visit_item_trait(inner, node));
        });
    }

    fn visit_item_impl(&mut self, node: &'ast ItemImpl) {
        let external = node
            .trait_
            .as_ref()
            .and_then(|(_, path, _)| path.segments.last())
            .is_some_and(|segment| {
                !self.catalog.traits.contains(&segment.ident.to_string())
            });
        let scope = if external {
            ParamScope::ExternalTrait
        } else {
            ParamScope::Checked
        };
        self.with_scope(scope, |scan| {
            scan.scoped(&node.generics, |inner| visit::visit_item_impl(inner, node));
        });
    }

    fn visit_signature(&mut self, node: &'ast Signature) {
        if matches!(self.scope, ParamScope::ExternalTrait) {
            return;
        }
        for argument in &node.inputs {
            if let FnArg::Typed(typed) = argument
                && let Pat::Ident(pattern) = &*typed.pat
            {
                self.check(Slot {
                    kind: Kind::Param,
                    owner: &[],
                    ident: &pattern.ident,
                    ty: &typed.ty,
                });
            }
        }
    }

    fn visit_local(&mut self, node: &'ast Local) {
        if let Pat::Type(typed) = &node.pat
            && let Pat::Ident(pattern) = &*typed.pat
        {
            self.check(Slot {
                kind: Kind::Let,
                owner: &[],
                ident: &pattern.ident,
                ty: &typed.ty,
            });
        } else if let Pat::Ident(pattern) = &node.pat
            && let Some(init) = &node.init
            && let Some(built) = self.built_type(&init.expr)
        {
            self.check(Slot {
                kind: Kind::Let,
                owner: &[],
                ident: &pattern.ident,
                ty: &built,
            });
        }
        visit::visit_local(self, node);
    }

    fn visit_expr_closure(&mut self, node: &'ast ExprClosure) {
        self.visit_expr(&node.body);
    }
}

fn parse_workspace() -> (Vec<(String, syn::File)>, Vec<String>) {
    let mut files = Vec::new();
    let mut failures = Vec::new();
    for (path, absolute) in support::source_files(&["src", "tests"]) {
        if path.starts_with("sifr-guards/") {
            continue;
        }
        match syn::parse_file(&support::read(&absolute)) {
            Ok(file) => files.push((path, file)),
            Err(error) => failures.push(format!("{path}: does not parse: {error}")),
        }
    }
    (files, failures)
}

fn compare(findings: &[Finding]) -> (Vec<String>, Vec<String>) {
    let mut remaining: BTreeMap<&str, usize> = BTreeMap::new();
    for line in BASELINE.lines().filter(|line| !line.is_empty()) {
        *remaining.entry(line).or_insert(0) += 1;
    }
    let mut violations = Vec::new();
    for finding in findings {
        if let Some(count) = remaining.get_mut(finding.key().as_str())
            && *count > 0
        {
            *count -= 1;
        } else {
            violations.push(finding.describe());
        }
    }
    let stale = remaining
        .into_iter()
        .flat_map(|(line, count)| std::iter::repeat_n(line.to_owned(), count))
        .collect();
    (violations, stale)
}

fn unsorted_baseline() -> Option<String> {
    let lines: Vec<&str> = BASELINE.lines().collect();
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    (lines != sorted).then(|| {
        "value_names_baseline.txt is not sorted; sort it with `LC_ALL=C sort`"
            .to_owned()
    })
}

#[test]
fn every_value_is_named_after_its_type() {
    let (files, mut violations) = parse_workspace();
    let mut catalog = Catalog::default();
    for (_, file) in &files {
        catalog.visit_file(file);
    }
    let mut findings = Vec::new();
    for (path, file) in &files {
        let mut mentions = Mentions {
            catalog: &catalog,
            words: BTreeSet::new(),
        };
        mentions.visit_file(file);
        let mut scan = Scan {
            catalog: &catalog,
            path,
            mentions: mentions.words,
            generics: Vec::new(),
            scope: ParamScope::Checked,
            findings: Vec::new(),
        };
        scan.visit_file(file);
        findings.append(&mut scan.findings);
    }
    let (new_violations, stale) = compare(&findings);
    violations.extend(new_violations);
    violations.extend(unsorted_baseline());
    support::report(
        "naming guard: a field, parameter or typed let is named after its type — the \
         type word, a role word from the glossary, or the type word without the \
         owner's own words; a domain word names only its own types. Fix the name; \
         the baseline only shrinks.",
        &violations,
        &stale,
    );
}
