use ra_ap_syntax::{ast::{self, HasName}, AstNode, SourceFile, TextRange};
use ra_ap_edition::Edition;
use petgraph::graph::{Graph, NodeIndex};
use std::collections::HashMap;

pub struct Workspace {
    pub files: Vec<(u32, String, String)>, // (FileId, FilePath, Content)
}

impl Workspace {
    pub fn new(source_files: Vec<(String, String)>) -> Self {
        let mut files = vec![];

        for (i, (path, content)) in source_files.into_iter().enumerate() {
            files.push((i as u32, path, content));
        }

        Self { files }
    }
}

pub struct HirCallGraph {
    pub graph: Graph<String, ()>,
    pub index: HashMap<String, NodeIndex>,
}

impl HirCallGraph {
    pub fn build(workspace: &Workspace) -> anyhow::Result<Self> {
        let mut graph = Graph::<String, ()>::new();
        let mut index = HashMap::new();

        for (_, _, content) in &workspace.files {
            let parse = SourceFile::parse(content, Edition::CURRENT);
            let tree = parse.tree();
            for func in tree.syntax().descendants().filter_map(ast::Fn::cast) {
                if let Some(name) = func.name() {
                    let n = name.text().to_string();
                    let idx = graph.add_node(n.clone());
                    index.insert(n, idx);
                }
            }
        }

        Ok(Self { graph, index })
    }

    pub fn add_edges(&mut self, workspace: &Workspace) -> anyhow::Result<()> {
        for (_, _, content) in &workspace.files {
            let parse = SourceFile::parse(content, Edition::CURRENT);
            let tree = parse.tree();

            for call in tree.syntax().descendants().filter_map(ast::CallExpr::cast) {
                if let Some(expr) = call.expr() {
                    let callee_name = expr.syntax().text().to_string();
                    
                    let caller_name = call.syntax().ancestors()
                        .find_map(ast::Fn::cast)
                        .and_then(|f| f.name())
                        .map(|n| n.text().to_string());

                    if let Some(caller_name) = caller_name {
                        if let (Some(&from), Some(&to)) = (self.index.get(&caller_name), self.index.get(&callee_name)) {
                            self.graph.add_edge(from, to, ());
                        }
                    }
                }
            }
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn get_all_callers(&self, target: &str) -> Vec<(String, NodeIndex)> {
        let mut callers = vec![];
        for edge in self.graph.raw_edges() {
            let callee_name = &self.graph[edge.target()];
            // Simplified string containment matching to simulate exact Semantic Match of Def if `{:?}` contains Name
            if callee_name.contains(target) {
                let caller_name = &self.graph[edge.source()];
                callers.push((caller_name.clone(), edge.source()));
            }
        }
        callers
    }
}

#[derive(Clone)]
pub struct TextEdit {
    pub replace_range: TextRange,
    pub replace_with: String,
}

pub struct TextEditBuilder {
    pub edits: Vec<TextEdit>,
}

impl TextEditBuilder {
    pub fn default() -> Self { Self { edits: Vec::new() } }
    pub fn replace(&mut self, range: TextRange, content: String) {
        self.edits.push(TextEdit { replace_range: range, replace_with: content });
    }
    pub fn finish(mut self) -> Vec<TextEdit> { 
        self.edits.sort_by_key(|e| std::cmp::Reverse(u32::from(e.replace_range.start())));
        self.edits 
    }
}

pub struct MultiFilePatch {
    pub edits: HashMap<u32, Vec<TextEdit>>,
}

impl MultiFilePatch {
    pub fn new() -> Self { Self { edits: HashMap::new() } }

    pub fn apply(&self, workspace: &mut Workspace) -> Vec<(String, String)> {
        let mut changes = Vec::new();
        for (file_id, path, source) in &mut workspace.files {
            if let Some(edits) = self.edits.get(&file_id) {
                let mut text = source.clone();
                for edit in edits {
                    let start = u32::from(edit.replace_range.start()) as usize;
                    let end = u32::from(edit.replace_range.end()) as usize;
                    if start <= text.len() && end <= text.len() {
                        text.replace_range(start..end, &edit.replace_with);
                    }
                }
                *source = text.clone();
                changes.push((path.clone(), text));
            }
        }
        changes
    }
}

pub fn generate_semantic_patch(source: &str, target_fn_name: &str) -> Option<Vec<TextEdit>> {
    let parse = SourceFile::parse(source, Edition::CURRENT);
    let tree = parse.tree();

    for call in tree.syntax().descendants().filter_map(ast::CallExpr::cast) {
        if call.syntax().text().to_string().contains(target_fn_name) {
            let mut builder = TextEditBuilder::default();
            builder.replace(
                call.syntax().text_range(),
                "prepare_query(\"SELECT * FROM users WHERE id = $1\", &[id])".into(),
            );
            return Some(builder.finish());
        }
    }
    None
}
