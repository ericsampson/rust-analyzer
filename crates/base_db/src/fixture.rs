//! A set of high-level utility fixture methods to use in tests.
use std::{mem, str::FromStr, sync::Arc};

use cfg::CfgOptions;
use rustc_hash::FxHashMap;
use test_utils::{
    extract_range_or_offset, Fixture, RangeOrOffset, CURSOR_MARKER, ESCAPED_CURSOR_MARKER,
};
use tt::Subtree;
use vfs::{file_set::FileSet, VfsPath};

use crate::{
    input::CrateName, Change, CrateDisplayName, CrateGraph, CrateId, Dependency, Edition, Env,
    FileId, FilePosition, FileRange, ProcMacro, ProcMacroExpander, ProcMacroExpansionError,
    SourceDatabaseExt, SourceRoot, SourceRootId,
};

pub const WORKSPACE: SourceRootId = SourceRootId(0);

pub trait WithFixture: Default + SourceDatabaseExt + 'static {
    fn with_single_file(text: &str) -> (Self, FileId) {
        let fixture = ChangeFixture::parse(text);
        let mut db = Self::default();
        fixture.change.apply(&mut db);
        assert_eq!(fixture.files.len(), 1);
        (db, fixture.files[0])
    }

    fn with_many_files(ra_fixture: &str) -> (Self, Vec<FileId>) {
        let fixture = ChangeFixture::parse(ra_fixture);
        let mut db = Self::default();
        fixture.change.apply(&mut db);
        assert!(fixture.file_position.is_none());
        (db, fixture.files)
    }

    fn with_files(ra_fixture: &str) -> Self {
        let fixture = ChangeFixture::parse(ra_fixture);
        let mut db = Self::default();
        fixture.change.apply(&mut db);
        assert!(fixture.file_position.is_none());
        db
    }

    fn with_position(ra_fixture: &str) -> (Self, FilePosition) {
        let (db, file_id, range_or_offset) = Self::with_range_or_offset(ra_fixture);
        let offset = range_or_offset.expect_offset();
        (db, FilePosition { file_id, offset })
    }

    fn with_range(ra_fixture: &str) -> (Self, FileRange) {
        let (db, file_id, range_or_offset) = Self::with_range_or_offset(ra_fixture);
        let range = range_or_offset.expect_range();
        (db, FileRange { file_id, range })
    }

    fn with_range_or_offset(ra_fixture: &str) -> (Self, FileId, RangeOrOffset) {
        let fixture = ChangeFixture::parse(ra_fixture);
        let mut db = Self::default();
        fixture.change.apply(&mut db);
        let (file_id, range_or_offset) = fixture
            .file_position
            .expect("Could not find file position in fixture. Did you forget to add an `$0`?");
        (db, file_id, range_or_offset)
    }

    fn test_crate(&self) -> CrateId {
        let crate_graph = self.crate_graph();
        let mut it = crate_graph.iter();
        let res = it.next().unwrap();
        assert!(it.next().is_none());
        res
    }
}

impl<DB: SourceDatabaseExt + Default + 'static> WithFixture for DB {}

pub struct ChangeFixture {
    pub file_position: Option<(FileId, RangeOrOffset)>,
    pub files: Vec<FileId>,
    pub change: Change,
}

impl ChangeFixture {
    pub fn parse(ra_fixture: &str) -> ChangeFixture {
        let (mini_core, proc_macros, fixture) = Fixture::parse(ra_fixture);
        let mut change = Change::new();

        let mut files = Vec::new();
        let mut crate_graph = CrateGraph::default();
        let mut crates = FxHashMap::default();
        let mut crate_deps = Vec::new();
        let mut default_crate_root: Option<FileId> = None;
        let mut default_cfg = CfgOptions::default();

        let mut file_set = FileSet::default();
        let mut current_source_root_kind = SourceRootKind::Local;
        let source_root_prefix = "/".to_string();
        let mut file_id = FileId(0);
        let mut roots = Vec::new();

        let mut file_position = None;

        for entry in fixture {
            let text = if entry.text.contains(CURSOR_MARKER) {
                if entry.text.contains(ESCAPED_CURSOR_MARKER) {
                    entry.text.replace(ESCAPED_CURSOR_MARKER, CURSOR_MARKER)
                } else {
                    let (range_or_offset, text) = extract_range_or_offset(&entry.text);
                    assert!(file_position.is_none());
                    file_position = Some((file_id, range_or_offset));
                    text
                }
            } else {
                entry.text.clone()
            };

            let meta = FileMeta::from(entry);
            assert!(meta.path.starts_with(&source_root_prefix));
            if !meta.deps.is_empty() {
                assert!(meta.krate.is_some(), "can't specify deps without naming the crate")
            }

            if let Some(kind) = &meta.introduce_new_source_root {
                let root = match current_source_root_kind {
                    SourceRootKind::Local => SourceRoot::new_local(mem::take(&mut file_set)),
                    SourceRootKind::Library => SourceRoot::new_library(mem::take(&mut file_set)),
                };
                roots.push(root);
                current_source_root_kind = *kind;
            }

            if let Some(krate) = meta.krate {
                let crate_name = CrateName::normalize_dashes(&krate);
                let crate_id = crate_graph.add_crate_root(
                    file_id,
                    meta.edition,
                    Some(crate_name.clone().into()),
                    meta.cfg.clone(),
                    meta.cfg,
                    meta.env,
                    Default::default(),
                );
                let prev = crates.insert(crate_name.clone(), crate_id);
                assert!(prev.is_none());
                for dep in meta.deps {
                    let prelude = meta.extern_prelude.contains(&dep);
                    let dep = CrateName::normalize_dashes(&dep);
                    crate_deps.push((crate_name.clone(), dep, prelude))
                }
            } else if meta.path == "/main.rs" || meta.path == "/lib.rs" {
                assert!(default_crate_root.is_none());
                default_crate_root = Some(file_id);
                default_cfg = meta.cfg;
            }

            change.change_file(file_id, Some(Arc::new(text)));
            let path = VfsPath::new_virtual_path(meta.path);
            file_set.insert(file_id, path);
            files.push(file_id);
            file_id.0 += 1;
        }

        if crates.is_empty() {
            let crate_root = default_crate_root
                .expect("missing default crate root, specify a main.rs or lib.rs");
            crate_graph.add_crate_root(
                crate_root,
                Edition::CURRENT,
                Some(CrateName::new("test").unwrap().into()),
                default_cfg.clone(),
                default_cfg,
                Env::default(),
                Default::default(),
            );
        } else {
            for (from, to, prelude) in crate_deps {
                let from_id = crates[&from];
                let to_id = crates[&to];
                crate_graph
                    .add_dep(
                        from_id,
                        Dependency::with_prelude(CrateName::new(&to).unwrap(), to_id, prelude),
                    )
                    .unwrap();
            }
        }

        if let Some(mini_core) = mini_core {
            let core_file = file_id;
            file_id.0 += 1;

            let mut fs = FileSet::default();
            fs.insert(core_file, VfsPath::new_virtual_path("/sysroot/core/lib.rs".to_string()));
            roots.push(SourceRoot::new_library(fs));

            change.change_file(core_file, Some(Arc::new(mini_core.source_code())));

            let all_crates = crate_graph.crates_in_topological_order();

            let core_crate = crate_graph.add_crate_root(
                core_file,
                Edition::Edition2021,
                Some(CrateDisplayName::from_canonical_name("core".to_string())),
                CfgOptions::default(),
                CfgOptions::default(),
                Env::default(),
                Vec::new(),
            );

            for krate in all_crates {
                crate_graph
                    .add_dep(krate, Dependency::new(CrateName::new("core").unwrap(), core_crate))
                    .unwrap();
            }
        }

        if !proc_macros.is_empty() {
            let proc_lib_file = file_id;
            file_id.0 += 1;

            let (proc_macro, source) = test_proc_macros(&proc_macros);
            let mut fs = FileSet::default();
            fs.insert(
                proc_lib_file,
                VfsPath::new_virtual_path("/sysroot/proc_macros/lib.rs".to_string()),
            );
            roots.push(SourceRoot::new_library(fs));

            change.change_file(proc_lib_file, Some(Arc::new(source)));

            let all_crates = crate_graph.crates_in_topological_order();

            let proc_macros_crate = crate_graph.add_crate_root(
                proc_lib_file,
                Edition::Edition2021,
                Some(CrateDisplayName::from_canonical_name("proc_macros".to_string())),
                CfgOptions::default(),
                CfgOptions::default(),
                Env::default(),
                proc_macro,
            );

            for krate in all_crates {
                crate_graph
                    .add_dep(
                        krate,
                        Dependency::new(CrateName::new("proc_macros").unwrap(), proc_macros_crate),
                    )
                    .unwrap();
            }
        }

        let root = match current_source_root_kind {
            SourceRootKind::Local => SourceRoot::new_local(mem::take(&mut file_set)),
            SourceRootKind::Library => SourceRoot::new_library(mem::take(&mut file_set)),
        };
        roots.push(root);
        change.set_roots(roots);
        change.set_crate_graph(crate_graph);

        ChangeFixture { file_position, files, change }
    }
}

fn test_proc_macros(proc_macros: &[String]) -> (Vec<ProcMacro>, String) {
    // The source here is only required so that paths to the macros exist and are resolvable.
    let source = r#"
#[proc_macro_attribute]
pub fn identity(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item
}
#[proc_macro_attribute]
pub fn input_replace(attr: TokenStream, _item: TokenStream) -> TokenStream {
    attr
}
#[proc_macro]
pub fn mirror(input: TokenStream) -> TokenStream {
    input
}
"#;
    let proc_macros = std::array::IntoIter::new([
        ProcMacro {
            name: "identity".into(),
            kind: crate::ProcMacroKind::Attr,
            expander: Arc::new(IdentityProcMacroExpander),
        },
        ProcMacro {
            name: "input_replace".into(),
            kind: crate::ProcMacroKind::Attr,
            expander: Arc::new(AttributeInputReplaceProcMacroExpander),
        },
        ProcMacro {
            name: "mirror".into(),
            kind: crate::ProcMacroKind::FuncLike,
            expander: Arc::new(MirrorProcMacroExpander),
        },
    ])
    .filter(|pm| proc_macros.iter().any(|name| name == pm.name))
    .collect();
    (proc_macros, source.into())
}

#[derive(Debug, Clone, Copy)]
enum SourceRootKind {
    Local,
    Library,
}

#[derive(Debug)]
struct FileMeta {
    path: String,
    krate: Option<String>,
    deps: Vec<String>,
    extern_prelude: Vec<String>,
    cfg: CfgOptions,
    edition: Edition,
    env: Env,
    introduce_new_source_root: Option<SourceRootKind>,
}

impl From<Fixture> for FileMeta {
    fn from(f: Fixture) -> FileMeta {
        let mut cfg = CfgOptions::default();
        f.cfg_atoms.iter().for_each(|it| cfg.insert_atom(it.into()));
        f.cfg_key_values.iter().for_each(|(k, v)| cfg.insert_key_value(k.into(), v.into()));

        let deps = f.deps;
        FileMeta {
            path: f.path,
            krate: f.krate,
            extern_prelude: f.extern_prelude.unwrap_or_else(|| deps.clone()),
            deps,
            cfg,
            edition: f.edition.as_ref().map_or(Edition::CURRENT, |v| Edition::from_str(v).unwrap()),
            env: f.env.into_iter().collect(),
            introduce_new_source_root: f.introduce_new_source_root.map(|kind| match &*kind {
                "local" => SourceRootKind::Local,
                "library" => SourceRootKind::Library,
                invalid => panic!("invalid source root kind '{}'", invalid),
            }),
        }
    }
}

// Identity mapping
#[derive(Debug)]
struct IdentityProcMacroExpander;
impl ProcMacroExpander for IdentityProcMacroExpander {
    fn expand(
        &self,
        subtree: &Subtree,
        _: Option<&Subtree>,
        _: &Env,
    ) -> Result<Subtree, ProcMacroExpansionError> {
        Ok(subtree.clone())
    }
}

// Pastes the attribute input as its output
#[derive(Debug)]
struct AttributeInputReplaceProcMacroExpander;
impl ProcMacroExpander for AttributeInputReplaceProcMacroExpander {
    fn expand(
        &self,
        _: &Subtree,
        attrs: Option<&Subtree>,
        _: &Env,
    ) -> Result<Subtree, ProcMacroExpansionError> {
        attrs
            .cloned()
            .ok_or_else(|| ProcMacroExpansionError::Panic("Expected attribute input".into()))
    }
}

#[derive(Debug)]
struct MirrorProcMacroExpander;
impl ProcMacroExpander for MirrorProcMacroExpander {
    fn expand(
        &self,
        input: &Subtree,
        _: Option<&Subtree>,
        _: &Env,
    ) -> Result<Subtree, ProcMacroExpansionError> {
        fn traverse(input: &Subtree) -> Subtree {
            let mut res = Subtree::default();
            res.delimiter = input.delimiter;
            for tt in input.token_trees.iter().rev() {
                let tt = match tt {
                    tt::TokenTree::Leaf(leaf) => tt::TokenTree::Leaf(leaf.clone()),
                    tt::TokenTree::Subtree(sub) => tt::TokenTree::Subtree(traverse(sub)),
                };
                res.token_trees.push(tt);
            }
            res
        }
        Ok(traverse(input))
    }
}
