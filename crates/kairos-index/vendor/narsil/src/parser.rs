use anyhow::{anyhow, Result};
use std::path::Path;
use std::sync::{Arc, OnceLock};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor, Tree};

use crate::symbols::{Symbol, SymbolKind};

/// Supported languages and their tree-sitter configurations
#[derive(Debug, Clone)]
pub struct LanguageConfig {
    pub name: String,
    pub language: Language,
    pub extensions: Vec<&'static str>,
    pub symbol_query: &'static str,
}

/// Language configuration with lazily-compiled query
/// Query is compiled on first use, warnings logged once, then cached
struct LazyLanguageConfig {
    config: LanguageConfig,
    /// Lazily compiled query (None if compilation failed)
    compiled_query: OnceLock<Option<Arc<Query>>>,
}

impl LazyLanguageConfig {
    fn new(config: LanguageConfig) -> Self {
        Self {
            config,
            compiled_query: OnceLock::new(),
        }
    }

    /// Get the compiled query, compiling on first access
    fn get_query(&self) -> Option<&Arc<Query>> {
        self.compiled_query
            .get_or_init(
                || match Query::new(&self.config.language, self.config.symbol_query) {
                    Ok(q) => Some(Arc::new(q)),
                    Err(e) => {
                        tracing::warn!(
                            "Query compilation failed for {} (this warning appears once): {:?}",
                            self.config.name,
                            e
                        );
                        None
                    }
                },
            )
            .as_ref()
    }
}

/// A parsed file with extracted information
#[derive(Debug, Clone)]
pub struct ParsedFile {
    /// Path of the parsed file (stored for reference, may be used by consumers)
    pub path: String,
    /// Language identifier for the file
    pub language: String,
    /// Symbols extracted from the file
    pub symbols: Vec<Symbol>,
    /// The tree-sitter parse tree (used for AST-aware chunking)
    pub tree: Option<Tree>,
}

/// Multi-language parser using tree-sitter
pub struct LanguageParser {
    configs: Vec<LazyLanguageConfig>,
}

impl LanguageParser {
    pub fn new() -> Result<Self> {
        let configs = vec![
            // Rust
            LanguageConfig {
                name: "rust".to_string(),
                language: tree_sitter_rust::LANGUAGE.into(),
                extensions: vec!["rs"],
                symbol_query: r#"
                    (function_item name: (identifier) @function.name) @function.def
                    ; KAIROS PATCH: keep a trait method with no body, the target of a generic call.
                    (function_signature_item name: (identifier) @function.name) @function.def
                    (struct_item name: (type_identifier) @struct.name) @struct.def
                    (enum_item name: (type_identifier) @enum.name) @enum.def
                    (trait_item name: (type_identifier) @trait.name) @trait.def
                    (impl_item type: (type_identifier) @impl.name) @impl.def
                    (type_item name: (type_identifier) @type.name) @type.def
                    (const_item name: (identifier) @const.name) @const.def
                    (static_item name: (identifier) @static.name) @static.def
                    (mod_item name: (identifier) @mod.name) @mod.def
                "#,
            },
            // Python
            LanguageConfig {
                name: "python".to_string(),
                language: tree_sitter_python::LANGUAGE.into(),
                extensions: vec!["py", "pyi"],
                symbol_query: r#"
                    (function_definition name: (identifier) @function.name) @function.def
                    (class_definition name: (identifier) @class.name) @class.def
                "#,
            },
            // TypeScript
            LanguageConfig {
                name: "typescript".to_string(),
                language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
                extensions: vec!["ts"],
                symbol_query: r#"
                    (function_declaration name: (identifier) @function.name) @function.def
                    (class_declaration name: (type_identifier) @class.name) @class.def
                    (method_definition name: (property_identifier) @method.name) @method.def
                    (interface_declaration name: (type_identifier) @interface.name) @interface.def
                    (type_alias_declaration name: (type_identifier) @type.name) @type.def
                    (enum_declaration name: (identifier) @enum.name) @enum.def
                "#,
            },
            // TSX
            LanguageConfig {
                name: "tsx".to_string(),
                language: tree_sitter_typescript::LANGUAGE_TSX.into(),
                extensions: vec!["tsx"],
                symbol_query: r#"
                    (function_declaration name: (identifier) @function.name) @function.def
                    (class_declaration name: (type_identifier) @class.name) @class.def
                    (method_definition name: (property_identifier) @method.name) @method.def
                    (interface_declaration name: (type_identifier) @interface.name) @interface.def
                    (type_alias_declaration name: (type_identifier) @type.name) @type.def
                "#,
            },
            // Go
            LanguageConfig {
                name: "go".to_string(),
                language: tree_sitter_go::LANGUAGE.into(),
                extensions: vec!["go"],
                symbol_query: r#"
                    (function_declaration name: (identifier) @function.name) @function.def
                    (method_declaration name: (field_identifier) @method.name) @method.def
                    (type_declaration (type_spec name: (type_identifier) @type.name)) @type.def
                "#,
            },
        ];

        // Wrap configs in lazy wrappers (queries compiled on first use, not during init)
        let lazy_configs = configs.into_iter().map(LazyLanguageConfig::new).collect();

        Ok(Self {
            configs: lazy_configs,
        })
    }

    /// Get language config for a file extension
    fn get_config(&self, path: &Path) -> Option<&LazyLanguageConfig> {
        let ext = path.extension()?.to_str()?;
        self.configs
            .iter()
            .find(|c| c.config.extensions.contains(&ext))
    }

    /// Parse a file and extract symbols
    pub fn parse_file(&self, path: &Path, content: &str) -> Result<ParsedFile> {
        let lazy_config = self
            .get_config(path)
            .ok_or_else(|| anyhow!("Unsupported file type: {:?}", path))?;

        let mut parser = Parser::new();
        parser.set_language(&lazy_config.config.language)?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse file"))?;

        let symbols = self.extract_symbols(&tree, content, lazy_config)?;

        Ok(ParsedFile {
            path: path.to_string_lossy().to_string(),
            language: lazy_config.config.name.clone(),
            symbols,
            tree: Some(tree),
        })
    }

    /// Parse a file and return just the tree (for call graph analysis)
    pub fn parse_to_tree(&self, path: &Path, content: &str) -> Result<Tree> {
        let lazy_config = self
            .get_config(path)
            .ok_or_else(|| anyhow!("Unsupported file type: {:?}", path))?;

        let mut parser = Parser::new();
        parser.set_language(&lazy_config.config.language)?;

        parser
            .parse(content, None)
            .ok_or_else(|| anyhow!("Failed to parse file"))
    }

    /// Extract symbols using tree-sitter queries
    fn extract_symbols(
        &self,
        tree: &Tree,
        source: &str,
        lazy_config: &LazyLanguageConfig,
    ) -> Result<Vec<Symbol>> {
        let mut symbols = Vec::new();
        let source_bytes = source.as_bytes();

        // Get lazily-compiled query (errors logged once on first access)
        let query = match lazy_config.get_query() {
            Some(q) => q,
            None => return Ok(symbols), // Query compilation failed, return empty
        };

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, tree.root_node(), source_bytes);

        while let Some(match_) = matches.next() {
            let mut name: Option<String> = None;
            let mut kind: Option<SymbolKind> = None;
            let mut start_line = 0;
            let mut end_line = 0;
            // KAIROS PATCH: the byte range of the definition node.
            let mut start_byte = 0;
            let mut end_byte = 0;
            let mut signature: Option<String> = None;

            for capture in match_.captures {
                let capture_name = query.capture_names()[capture.index as usize];
                let node = capture.node;
                let text = node.utf8_text(source_bytes).unwrap_or("");

                if capture_name.ends_with(".name") {
                    name = Some(text.to_string());
                    kind = Some(parse_symbol_kind(capture_name));
                } else if capture_name.ends_with(".def") {
                    start_line = node.start_position().row + 1;
                    end_line = node.end_position().row + 1;
                    // KAIROS PATCH: the byte range of the definition node.
                    start_byte = node.start_byte();
                    end_byte = node.end_byte();

                    // Extract first line as signature (safe byte boundary)
                    let first_line_end = text.find('\n').unwrap_or(text.len());
                    let sig_end = text.floor_char_boundary(first_line_end.min(200));
                    signature = Some(text[..sig_end].to_string());
                }
            }

            if let (Some(name), Some(kind)) = (name, kind) {
                symbols.push(Symbol {
                    name,
                    kind,
                    file_path: String::new(), // Will be set by caller
                    start_line,
                    end_line,
                    start_byte, // KAIROS PATCH
                    end_byte,   // KAIROS PATCH
                    signature,
                    qualified_name: None,
                    doc_comment: None,
                });
            }
        }

        Ok(symbols)
    }
}

fn parse_symbol_kind(capture_name: &str) -> SymbolKind {
    let prefix = capture_name.split('.').next().unwrap_or("");
    match prefix {
        "function" => SymbolKind::Function,
        "method" => SymbolKind::Method,
        "class" => SymbolKind::Class,
        "struct" => SymbolKind::Struct,
        "enum" => SymbolKind::Enum,
        "interface" => SymbolKind::Interface,
        "trait" => SymbolKind::Trait,
        "type" => SymbolKind::TypeAlias,
        "const" | "static" => SymbolKind::Constant,
        "mod" | "module" | "namespace" => SymbolKind::Module,
        "impl" => SymbolKind::Implementation,
        "var" | "arrow" => SymbolKind::Variable,
        _ => SymbolKind::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rust() {
        let parser = LanguageParser::new().unwrap();
        let content = r#"
            pub struct MyStruct {
                field: u32,
            }

            pub fn my_function() -> i32 {
                42
            }

            impl MyStruct {
                pub fn method(&self) {}
            }
        "#;

        let parsed = parser.parse_file(Path::new("test.rs"), content).unwrap();
        assert_eq!(parsed.language, "rust");
        assert!(!parsed.symbols.is_empty());

        let names: Vec<_> = parsed.symbols.iter().map(|s| &s.name).collect();
        assert!(names.contains(&&"MyStruct".to_string()));
        assert!(names.contains(&&"my_function".to_string()));
    }

    #[test]
    fn test_parse_python() {
        let parser = LanguageParser::new().unwrap();
        let content = r#"
class MyClass:
    def __init__(self):
        pass

    def method(self):
        return 42

def standalone_function():
    pass
        "#;

        let parsed = parser.parse_file(Path::new("test.py"), content).unwrap();
        assert_eq!(parsed.language, "python");

        let names: Vec<_> = parsed.symbols.iter().map(|s| &s.name).collect();
        assert!(names.contains(&&"MyClass".to_string()));
        assert!(names.contains(&&"standalone_function".to_string()));
    }
}
