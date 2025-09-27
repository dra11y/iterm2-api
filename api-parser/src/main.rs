use anyhow::{Result, anyhow};
use chrono::{self, Utc};
use clap::{Parser, Subcommand, ValueEnum};
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use std::{fs, fs::read_dir, io::Write, path::Path};
use tokio::task::JoinHandle;
use tracing::{debug, info};
use tracing_subscriber::fmt;
use tree_sitter::{Node, Parser as TreeParser};

#[derive(Debug, Default, ValueEnum, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[clap(rename_all = "lower")]
enum OutFormat {
    #[default]
    Json,
    Csv,
    Markdown,
}

#[derive(Parser)]
#[command(
    name = "python-parser",
    about = "Extracts and queries iTerm2 Python API structure from source code",
    version = "0.1.0"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to Python source directory
    #[clap(short, long, default_value = "iTerm2/api/library/python/iterm2")]
    source: String,

    /// Export format (json, csv, markdown)
    #[clap(short, long, default_value = "json")]
    format: OutFormat,

    /// Enable verbose logging
    #[clap(short, long)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// List all classes and their methods
    List {
        /// Filter by class name pattern
        #[clap(short, long)]
        class: Option<String>,

        /// Show detailed information including docstrings
        #[clap(long)]
        detailed: bool,
    },

    /// Query specific classes and their methods
    Query {
        /// Class name to query (required)
        #[clap(short, long)]
        class: String,

        /// Filter methods by name pattern
        #[clap(short, long)]
        method: Option<String>,

        /// Filter by parameter name
        #[clap(long)]
        parameter: Option<String>,

        /// Show method signatures only
        #[clap(long)]
        signatures: bool,

        /// Show full docstrings
        #[clap(long)]
        docs: bool,
    },

    /// Search for functions across all modules
    Functions {
        /// Filter by function name pattern
        #[clap(short, long)]
        name: Option<String>,

        /// Filter by parameter name
        #[clap(long)]
        parameter: Option<String>,

        /// Show async functions only
        #[clap(long)]
        async_only: bool,
    },

    /// Show API statistics
    Stats {
        /// Include detailed method analysis
        #[clap(long)]
        detailed: bool,
    },

    /// Extract API structure to stdout
    Extract {
        /// Include enums and functions
        #[clap(long)]
        full: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PythonApi {
    classes: Vec<PythonClass>,
    enums: Vec<PythonEnum>,
    functions: Vec<PythonFunction>,
    metadata: ApiMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApiMetadata {
    total_files: usize,
    total_classes: usize,
    total_functions: usize,
    total_enums: usize,
    source_directory: String,
    extraction_timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PythonClass {
    name: String,
    file_path: String,
    docstring: Option<String>,
    methods: Vec<PythonMethod>,
    properties: Vec<PythonProperty>,
    inherits: Vec<String>,
    line_number: Option<u32>,
    is_exception: bool,
    is_abstract: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PythonMethod {
    name: String,
    signature: String,
    docstring: Option<String>,
    parameters: Vec<Parameter>,
    returns: String,
    is_async: bool,
    is_static: bool,
    decorators: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PythonProperty {
    name: String,
    type_hint: String,
    docstring: Option<String>,
    is_readonly: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PythonEnum {
    name: String,
    file_path: String,
    docstring: Option<String>,
    values: Vec<EnumValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EnumValue {
    name: String,
    value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PythonFunction {
    name: String,
    file_path: String,
    signature: String,
    docstring: Option<String>,
    parameters: Vec<Parameter>,
    returns: String,
    is_async: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Parameter {
    name: String,
    type_hint: String,
    default_value: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    if cli.verbose {
        fmt::init();
    } else {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .init();
    }

    info!("Parsing Python API from: {}", cli.source);
    let api = parse_python_api(&cli.source).await?;

    // Handle different commands
    match cli.command {
        Commands::List { class, detailed } => {
            let output = execute_list_command(&api, class, detailed, cli.format)?;
            println!("{output}");
        }
        Commands::Query {
            class,
            method,
            parameter,
            signatures,
            docs,
        } => {
            let output = execute_query_command(
                &api, &class, method, parameter, signatures, docs, cli.format,
            )?;
            println!("{output}");
        }
        Commands::Functions {
            name,
            parameter,
            async_only,
        } => {
            let output = execute_functions_command(&api, name, parameter, async_only, cli.format)?;
            println!("{output}");
        }
        Commands::Stats { detailed } => {
            let stats = generate_stats(&api, detailed)?;
            println!("{stats}");
        }
        Commands::Extract { full } => {
            let structure = extract_api_structure(&api, full)?;
            println!("{structure}");
        }
    }

    Ok(())
}

async fn parse_python_api(source_path: &str) -> Result<PythonApi> {
    let source_dir = Path::new(source_path);
    if !source_dir.exists() {
        return Err(anyhow!("Source directory does not exist: {source_path}"));
    }

    let mut classes = Vec::new();
    let mut enums = Vec::new();
    let mut functions = Vec::new();
    let mut total_files = 0;

    // Parse all Python files in the directory recursively
    let mut parse_futures = Vec::new();

    fn collect_python_files(
        dir: &Path,
        parse_futures: &mut Vec<JoinHandle<Option<FileApi>>>,
        total_files: &mut usize,
        source_dir: &Path,
    ) -> Result<()> {
        for entry in read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();

            // Handle directories recursively
            if path.is_dir() {
                collect_python_files(&path, parse_futures, total_files, source_dir)?;
                continue;
            }

            // Skip non-Python files
            if path.extension().and_then(|s| s.to_str()) != Some("py") {
                continue;
            }

            // Get file name and skip hidden files (except __init__.py)
            let Some(file_name) = path.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            
            if file_name.starts_with('_') && file_name != "__init__.py" {
                continue;
            }

            // Process valid Python file
            *total_files += 1;
            let file_path = path.clone();
            let file_name_clone = file_name.to_string();
            info!("Parsing file: {}", file_path.display());
            let source_dir_clone = source_dir.to_path_buf();
            parse_futures.push(tokio::spawn(async move {
                let file_start = std::time::Instant::now();
                let result = match parse_python_file(&file_path, &source_dir_clone).await {
                    Ok(file_api) => Some(file_api),
                    Err(e) => {
                        debug!("Failed to parse {file_name_clone}: {e}");
                        None
                    }
                };
                let file_duration = file_start.elapsed();
                if file_duration.as_millis() > 100 {
                    debug!("Slow file parse: {file_name_clone} took {file_duration:?}");
                }
                result
            }));
        }
        Ok(())
    }

    collect_python_files(source_dir, &mut parse_futures, &mut total_files, source_dir)?;

    // Wait for all parsing to complete
    let start_time = std::time::Instant::now();
    debug!("Waiting for parsing...");
    let results = join_all(parse_futures).await;
    let join_duration = start_time.elapsed();
    debug!("Parsing complete! join_all took: {join_duration:?}");

    for result in results {
        match result {
            Ok(Some(file_api)) => {
                classes.extend(file_api.classes);
                enums.extend(file_api.enums);
                functions.extend(file_api.functions);
            }
            Ok(None) => {
                debug!("File parsing failed");
                // File parsing failed, already logged in the task
            }
            Err(join_error) => {
                if join_error.is_panic() {
                    debug!("Task panicked");
                } else {
                    debug!("Task failed: {join_error}");
                }
            }
        }
    }

    let total_classes = classes.len();
    let total_functions = functions.len();
    let total_enums = enums.len();

    Ok(PythonApi {
        classes,
        enums,
        functions,
        metadata: ApiMetadata {
            total_files,
            total_classes,
            total_functions,
            total_enums,
            source_directory: source_path.to_string(),
            extraction_timestamp: Utc::now().to_rfc3339(),
        },
    })
}

#[derive(Debug, Serialize, Deserialize)]
struct FileApi {
    classes: Vec<PythonClass>,
    enums: Vec<PythonEnum>,
    functions: Vec<PythonFunction>,
}

async fn parse_python_file(file_path: &Path, _source_dir: &Path) -> Result<FileApi> {
    debug!("parse_python_file: {}", file_path.display());

    // Read the file content
    let source_code = match fs::read_to_string(file_path) {
        Ok(content) => content,
        Err(e) => {
            debug!("Failed to read file {}: {e}", file_path.display());
            return Ok(FileApi {
                classes: Vec::new(),
                enums: Vec::new(),
                functions: Vec::new(),
            });
        }
    };

    // Parse using tree-sitter
    let mut parser = TreeParser::new();
    parser
        .set_language(&tree_sitter_python::LANGUAGE.into())
        .map_err(|e| anyhow!("Failed to set language: {e}"))?;

    let tree = match parser.parse(&source_code, None) {
        Some(tree) => tree,
        None => {
            debug!("Failed to parse file {}: syntax error", file_path.display());
            return Ok(FileApi {
                classes: Vec::new(),
                enums: Vec::new(),
                functions: Vec::new(),
            });
        }
    };

    info!("parse_python_file SUCCESS: {}", file_path.display());

    let mut classes = Vec::new();
    let mut enums = Vec::new();
    let mut functions = Vec::new();

    // Find all class definitions
    let root_node = tree.root_node();
    find_class_definitions(
        &source_code,
        &root_node,
        file_path,
        &mut classes,
        &mut enums,
    )?;

    // Find all function definitions (not inside classes)
    find_function_definitions(&source_code, &root_node, file_path, &mut functions)?;

    let result = FileApi {
        classes,
        enums,
        functions,
    };

    Ok(result)
}

fn find_class_definitions(
    source_code: &str,
    node: &Node,
    file_path: &Path,
    classes: &mut Vec<PythonClass>,
    enums: &mut Vec<PythonEnum>,
) -> Result<()> {
    if node.kind() == "class_definition" {
        match parse_class_definition(source_code, node, file_path) {
            Ok(class) => {
                // Check if it's an enum (inherits from Enum)
                if class
                    .inherits
                    .iter()
                    .any(|superclass| superclass == "Enum" || superclass.ends_with("Enum"))
                {
                    enums.push(convert_class_to_enum(class));
                } else {
                    classes.push(class);
                }
            }
            Err(e) => debug!(
                "Failed to parse class definition in {}: {e}",
                file_path.display()
            ),
        }
    }

    // Recursively search child nodes
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            find_class_definitions(source_code, &child, file_path, classes, enums)?;
        }
    }

    Ok(())
}

fn find_function_definitions(
    source_code: &str,
    node: &Node,
    file_path: &Path,
    functions: &mut Vec<PythonFunction>,
) -> Result<()> {
    if node.kind() == "function_definition" {
        // Check if this function is not inside a class
        if !is_inside_class_node(node) {
            match parse_function_definition(source_code, node, file_path) {
                Ok(func) => functions.push(func),
                Err(e) => debug!(
                    "Failed to parse function definition in {}: {e}",
                    file_path.display()
                ),
            }
        }
    }

    // Recursively search child nodes
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            find_function_definitions(source_code, &child, file_path, functions)?;
        }
    }

    Ok(())
}

fn is_inside_class_node(node: &Node) -> bool {
    let mut parent = node.parent();
    while let Some(current) = parent {
        if current.kind() == "class_definition" {
            return true;
        }
        parent = current.parent();
    }
    false
}

fn convert_class_to_enum(class: PythonClass) -> PythonEnum {
    PythonEnum {
        name: class.name.clone(),
        file_path: class.file_path.clone(),
        docstring: class.docstring.clone(),
        values: extract_enum_values_from_class(&class),
    }
}

fn parse_class_definition(
    source_code: &str,
    class_node: &Node,
    file_path: &Path,
) -> Result<PythonClass> {
    let mut methods = Vec::new();
    let mut properties = Vec::new();

    // Extract class name
    let name = extract_node_name(class_node, source_code)?;

    // Extract inheritance
    let inherits = extract_superclasses(class_node, source_code)?;

    // Find methods and properties inside this class
    find_class_members(source_code, class_node, &mut methods, &mut properties)?;

    let is_exception = inherits.contains(&"Exception".to_string());
    let is_abstract =
        inherits.contains(&"ABC".to_string()) || inherits.iter().any(|s| s.contains("Abstract"));

    Ok(PythonClass {
        name,
        file_path: file_path.to_string_lossy().to_string(),
        docstring: extract_docstring_from_node(source_code, class_node),
        methods,
        properties,
        inherits,
        line_number: Some((class_node.start_position().row + 1).try_into().unwrap()),
        is_exception,
        is_abstract,
    })
}

fn extract_node_name(node: &Node, source_code: &str) -> Result<String> {
    // Find the identifier child node
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "identifier" {
                return Ok(source_code[child.start_byte()..child.end_byte()].to_string());
            }
        }
    }
    Err(anyhow!("Could not extract name from node"))
}

fn extract_superclasses(node: &Node, source_code: &str) -> Result<Vec<String>> {
    let mut superclasses = Vec::new();

    // Look for argument_list node (superclasses)
    for i in 0..node.child_count() {
        let Some(child) = node.child(i) else { continue };
        if child.kind() != "argument_list" { continue; }
        
        // Extract identifiers from argument list
        for j in 0..child.child_count() {
            let Some(arg) = child.child(j) else { continue };
            if arg.kind() == "identifier" {
                superclasses.push(source_code[arg.start_byte()..arg.end_byte()].to_string());
            }
        }
    }

    Ok(superclasses)
}

fn find_class_members(
    source_code: &str,
    class_node: &Node,
    methods: &mut Vec<PythonMethod>,
    properties: &mut Vec<PythonProperty>,
) -> Result<()> {
    // Find the class body (block node)
    for i in 0..class_node.child_count() {
        let Some(child) = class_node.child(i) else { continue };
        if child.kind() != "block" { continue; }
        
        // Search for methods and properties in the block
        for j in 0..child.child_count() {
            let Some(member) = child.child(j) else { continue };
            
            match member.kind() {
                "function_definition" => {
                    if let Ok(method) = parse_method_definition(source_code, &member) {
                        methods.push(method);
                    }
                }
                "decorated_definition" => {
                    if let Ok(property) = parse_property_definition(source_code, &member) {
                        properties.push(property);
                    }
                }
                _ => {}
            }
        }
        break;
    }

    Ok(())
}

fn parse_method_definition(source_code: &str, method_node: &Node) -> Result<PythonMethod> {
    let name = extract_node_name(method_node, source_code)?;
    let parameters = extract_parameters_from_node(method_node, source_code);
    let is_async = is_async_function_node(method_node);
    let is_static = is_static_method_node(method_node);
    let decorators = extract_decorators_from_node(method_node, source_code);

    Ok(PythonMethod {
        name,
        signature: extract_signature_from_node(method_node, source_code),
        docstring: extract_docstring_from_node(source_code, method_node),
        parameters,
        returns: extract_return_type_from_node(method_node, source_code),
        is_async,
        is_static,
        decorators,
    })
}

fn parse_function_definition(
    source_code: &str,
    func_node: &Node,
    file_path: &Path,
) -> Result<PythonFunction> {
    let name = extract_node_name(func_node, source_code)?;
    let parameters = extract_parameters_from_node(func_node, source_code);
    let is_async = is_async_function_node(func_node);

    Ok(PythonFunction {
        name,
        file_path: file_path.to_string_lossy().to_string(),
        signature: extract_signature_from_node(func_node, source_code),
        docstring: extract_docstring_from_node(source_code, func_node),
        parameters,
        returns: extract_return_type_from_node(func_node, source_code),
        is_async,
    })
}

fn parse_property_definition(source_code: &str, property_node: &Node) -> Result<PythonProperty> {
    let name = extract_node_name(property_node, source_code)?;

    Ok(PythonProperty {
        name,
        type_hint: extract_return_type_from_node(property_node, source_code),
        docstring: extract_docstring_from_node(source_code, property_node),
        is_readonly: !has_setter_node(property_node, source_code),
    })
}

// Helper functions for extraction
fn extract_docstring_from_node(source_code: &str, node: &Node) -> Option<String> {
    // Look for string literals within the node's body
    let mut string_literals = Vec::new();

    // Find the body block of the function/class
    let Some(body) = find_body_node(node) else { return None };

    // Look for string literals in the body
    for i in 0..body.child_count() {
        let Some(child) = body.child(i) else { continue };
        if child.kind() != "expression_statement" { continue; }
        
        for j in 0..child.child_count() {
            let Some(expr) = child.child(j) else { continue };
            if expr.kind() == "string" {
                string_literals.push(expr);
            }
        }
    }

    // Take the first string literal (likely the docstring)
    let Some(string_node) = string_literals.first() else { return None };
    let string_content = &source_code[string_node.start_byte()..string_node.end_byte()];

    // Remove quotes and clean up
    let content = if string_content.starts_with("\"\"\"") && string_content.ends_with("\"\"\"") {
        &string_content[3..string_content.len() - 3]
    } else if string_content.starts_with("'''") && string_content.ends_with("'''") {
        &string_content[3..string_content.len() - 3]
    } else if string_content.starts_with('"') && string_content.ends_with('"') {
        &string_content[1..string_content.len() - 1]
    } else if string_content.starts_with('\'') && string_content.ends_with('\'') {
        &string_content[1..string_content.len() - 1]
    } else {
        return None;
    };
    
    Some(content.trim().to_string())
}

fn find_body_node<'a>(node: &'a Node) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    node
        .children(&mut cursor)
        .find(|child| child.kind() == "block")
}

fn extract_parameters_from_node(node: &Node, source_code: &str) -> Vec<Parameter> {
    let mut parameters = Vec::new();

    // Find the parameters node
    for i in 0..node.child_count() {
        let Some(child) = node.child(i) else { continue };
        if child.kind() != "parameters" { continue; }
        
        // Extract individual parameters
        for j in 0..child.child_count() {
            let Some(param) = child.child(j) else { continue };
            if param.kind() != "identifier" { continue; }
            
            let param_name = source_code[param.start_byte()..param.end_byte()].to_string();

            // Skip 'self' and 'cls' parameters
            if param_name == "self" || param_name == "cls" {
                continue;
            }

            // Look for type annotation
            let type_hint = find_type_annotation(&param, source_code);

            parameters.push(Parameter {
                name: param_name,
                type_hint,
                default_value: extract_default_value_from_param(
                    &param,
                    source_code,
                ),
            });
        }
    }

    parameters
}

fn find_type_annotation(node: &Node, source_code: &str) -> String {
    // Look for type annotation (usually the next sibling after identifier)
    let mut parent = node.parent();
    while let Some(current) = parent {
        for i in 0..current.child_count() {
            if let Some(child) = current.child(i) {
                if child.kind() == "type" {
                    return source_code[child.start_byte()..child.end_byte()].to_string();
                }
            }
        }
        parent = current.parent();
    }
    "Any".to_string()
}

fn extract_signature_from_node(node: &Node, source_code: &str) -> String {
    let name = extract_node_name(node, source_code).unwrap_or_default();
    let parameters = extract_parameters_from_node(node, source_code);

    let param_str = parameters
        .iter()
        .map(|p| {
            if let Some(default) = &p.default_value {
                format!("{}: {} = {default}", p.name, p.type_hint)
            } else if !p.type_hint.is_empty() {
                format!("{}: {}", p.name, p.type_hint)
            } else {
                p.name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");

    format!("{name}({param_str})")
}

fn extract_return_type_from_node(node: &Node, source_code: &str) -> String {
    // Look for return type annotation (usually after parameters)
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "type" {
                return source_code[child.start_byte()..child.end_byte()].to_string();
            }
        }
    }
    "Any".to_string()
}

fn is_async_function_node(node: &Node) -> bool {
    // Check if the function has an async modifier
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            if child.kind() == "async" {
                return true;
            }
        }
    }
    false
}

fn is_static_method_node(node: &Node) -> bool {
    // Check if the method has @staticmethod decorator
    let decorators = extract_decorators_from_node(node, "");
    decorators.contains(&"staticmethod".to_string())
}

fn extract_decorators_from_node(node: &Node, source_code: &str) -> Vec<String> {
    let mut decorators = Vec::new();

    // Look for decorator nodes in parent decorated_definition
    let mut parent = node.parent();
    while let Some(current) = parent {
        if current.kind() != "decorated_definition" {
            parent = current.parent();
            continue;
        }
        
        for i in 0..current.child_count() {
            let Some(child) = current.child(i) else { continue };
            if child.kind() != "decorator" { continue; }
            
            // Extract decorator name
            for j in 0..child.child_count() {
                let Some(dec_child) = child.child(j) else { continue };
                if dec_child.kind() != "identifier" { continue; }
                
                decorators.push(extract_decorator_name(&dec_child, source_code));
            }
        }
        break;
    }

    decorators
}

fn has_setter_node(node: &Node, source_code: &str) -> bool {
    // Look for setter decorator in parent decorated_definition
    let mut parent = node.parent();
    while let Some(current) = parent {
        if current.kind() != "decorated_definition" {
            parent = current.parent();
            continue;
        }

        for i in 0..current.child_count() {
            let Some(child) = current.child(i) else { continue };
            if child.kind() != "decorator" { continue; }

            // Check if this is a setter decorator
            for j in 0..child.child_count() {
                let Some(dec_child) = child.child(j) else { continue };
                if dec_child.kind() != "identifier" { continue; }

                let decorator_name = source_code
                    [dec_child.start_byte()..dec_child.end_byte()]
                    .to_string();
                if decorator_name == "setter" {
                    return true;
                }
            }
        }
        break;
    }
    false
}

// Command implementations
fn execute_list_command(
    api: &PythonApi,
    class_filter: Option<String>,
    detailed: bool,
    format: OutFormat,
) -> Result<String> {
    let mut filtered_classes = api.classes.clone();

    // Apply class filter if provided
    debug!("Found {} classes before filtering", filtered_classes.len());
    for class in &filtered_classes {
        debug!("Found class: {}", class.name);
    }

    if let Some(filter) = &class_filter {
        debug!("Applying filter: '{filter}'");
        filtered_classes.retain(|class| class.name.to_lowercase().contains(&filter.to_lowercase()));
        debug!("Found {} classes after filtering", filtered_classes.len());
    }

    // Sort classes by name
    filtered_classes.sort_by(|a, b| a.name.cmp(&b.name));

    let output = if detailed {
        output_detailed_classes(&filtered_classes, format)?
    } else {
        output_class_summary(&filtered_classes, format)?
    };

    Ok(output)
}

fn execute_query_command(
    api: &PythonApi,
    class_name: &str,
    method_filter: Option<String>,
    parameter_filter: Option<String>,
    signatures_only: bool,
    show_docs: bool,
    format: OutFormat,
) -> Result<String> {
    // Find the specific class, excluding enum classes from mainmenu.py
    let class = api
        .classes
        .iter()
        .filter(|c| {
            c.name.to_lowercase() == class_name.to_lowercase() &&
            // Exclude classes from mainmenu.py (these are menu identifiers, not API classes)
            !c.file_path.contains("mainmenu.py")
        })
        .next()
        .ok_or_else(|| anyhow!("Class '{class_name}' not found"))?;

    let mut methods = class.methods.clone();

    // Apply method filter if provided
    if let Some(filter) = &method_filter {
        methods.retain(|method| method.name.to_lowercase().contains(&filter.to_lowercase()));
    }

    // Apply parameter filter if provided
    if let Some(filter) = &parameter_filter {
        methods.retain(|method| {
            method
                .parameters
                .iter()
                .any(|param| param.name.to_lowercase().contains(&filter.to_lowercase()))
        });
    }

    // Sort methods by name
    methods.sort_by(|a, b| a.name.cmp(&b.name));

    let output = if signatures_only {
        output_method_signatures(&class.name, &methods, format)?
    } else if show_docs {
        output_class_with_docs(class, &methods, format)?
    } else {
        output_class_methods(&class.name, &methods, format)?
    };

    Ok(output)
}

fn execute_functions_command(
    api: &PythonApi,
    name_filter: Option<String>,
    parameter_filter: Option<String>,
    async_only: bool,
    format: OutFormat,
) -> Result<String> {
    let mut functions = api.functions.clone();

    // Apply name filter if provided
    if let Some(filter) = &name_filter {
        functions.retain(|func| func.name.to_lowercase().contains(&filter.to_lowercase()));
    }

    // Apply async filter if provided
    if async_only {
        functions.retain(|func| func.is_async);
    }

    // Apply parameter filter if provided
    if let Some(filter) = &parameter_filter {
        functions.retain(|func| {
            func.parameters
                .iter()
                .any(|param| param.name.to_lowercase().contains(&filter.to_lowercase()))
        });
    }

    // Sort functions by name
    functions.sort_by(|a, b| a.name.cmp(&b.name));

    let output = output_functions(&functions, format)?;

    Ok(output)
}

fn generate_stats(api: &PythonApi, detailed: bool) -> Result<String> {
    let stats = if detailed {
        generate_detailed_stats(api)?
    } else {
        generate_simple_stats(api)?
    };

    Ok(stats)
}

fn extract_api_structure(api: &PythonApi, full: bool) -> Result<String> {
    let structure = if full {
        serde_json::to_string_pretty(api)?
    } else {
        // Extract only classes and methods for basic structure
        let simplified = PythonApi {
            classes: api.classes.clone(),
            // Skip enums in basic mode
            enums: Vec::new(),
            // Skip functions in basic mode
            functions: Vec::new(),
            metadata: api.metadata.clone(),
        };
        serde_json::to_string_pretty(&simplified)?
    };

    Ok(structure)
}

// Output functions for different commands
fn output_class_summary(classes: &[PythonClass], format: OutFormat) -> Result<String> {
    let class_summary = match format {
        OutFormat::Json => {
            let summary: Vec<_> = classes
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "name": c.name,
                        "methods": c.methods.len(),
                        "file": c.file_path,
                        "line": c.line_number,
                        "inherits": c.inherits,
                        "is_exception": c.is_exception,
                        "is_abstract": c.is_abstract,
                    })
                })
                .collect();
            serde_json::to_string_pretty(&summary)?
        }
        OutFormat::Csv => {
            let mut csv = Vec::new();
            writeln!(csv, "Class,Methods,File,Line,Inherits,Exception,Abstract")?;
            for class in classes {
                writeln!(
                    csv,
                    "{},{},{},{},{},{},{}",
                    class.name,
                    class.methods.len(),
                    class.file_path,
                    class.line_number.unwrap_or(0),
                    class.inherits.join(";"),
                    class.is_exception,
                    class.is_abstract
                )?;
            }
            String::from_utf8_lossy(&csv).to_string()
        }
        OutFormat::Markdown => {
            let mut md = Vec::new();
            writeln!(md, "# iTerm2 API Classes\n")?;
            writeln!(md, "| Class | Methods | File | Line | Inherits |")?;
            writeln!(md, "|-------|---------|------|------|----------|")?;
            for class in classes {
                writeln!(
                    md,
                    "| `{}` | {} | `{}` | {} | {} |",
                    class.name,
                    class.methods.len(),
                    class.file_path,
                    class.line_number.unwrap_or(0),
                    if class.inherits.is_empty() {
                        "-".to_string()
                    } else {
                        class.inherits.join(", ")
                    }
                )?;
            }
            String::from_utf8_lossy(&md).to_string()
        }
    };

    Ok(class_summary)
}

fn output_detailed_classes(classes: &[PythonClass], format: OutFormat) -> Result<String> {
    let detailed_classes = match format {
        OutFormat::Json => serde_json::to_string_pretty(classes)?,
        OutFormat::Markdown => {
            let mut md = Vec::new();
            writeln!(md, "# iTerm2 API Classes (Detailed)\n")?;
            for class in classes {
                writeln!(md, "## Class: `{}`\n", class.name)?;
                writeln!(md, "**File**: `{}`", class.file_path)?;
                writeln!(md, "**Line**: {}", class.line_number.unwrap_or(0))?;
                if !class.inherits.is_empty() {
                    writeln!(md, "**Inherits**: {}", class.inherits.join(", "))?;
                }
                writeln!(md, "**Methods**: {}\n", class.methods.len())?;

                if class.methods.is_empty() {
                    writeln!(md)?;
                    continue;
                }

                writeln!(md, "### Methods\n")?;
                for method in &class.methods {
                    writeln!(md, "- `{}`", method.signature)?;
                    
                    let Some(docstring) = &method.docstring else { continue };
                    if docstring.is_empty() { continue; }
                    
                    let doc_preview = docstring.lines().next().unwrap_or("");
                    if doc_preview.is_empty() { continue; }
                    
                    writeln!(md, "  - *{doc_preview}*")?;
                }
                writeln!(md)?;
            }
            String::from_utf8_lossy(&md).to_string()
        }
        OutFormat::Csv => {
            let mut csv = Vec::new();
            writeln!(
                csv,
                "Class,Method,Signature,Parameters,Async,Static,Docstring"
            )?;
            for class in classes {
                for method in &class.methods {
                    let params: Vec<String> = method
                        .parameters
                        .iter()
                        .map(|p| format!("{}: {}", p.name, p.type_hint))
                        .collect();
                    writeln!(
                        csv,
                        "\"{}\",\"{}\",\"{}\",\"{}\",{},{},\"{}\"",
                        class.name,
                        method.name,
                        method.signature,
                        params.join("; "),
                        method.is_async,
                        method.is_static,
                        method
                            .docstring
                            .as_deref()
                            .unwrap_or("")
                            .replace('"', "\"\"")
                    )?;
                }
            }
            String::from_utf8_lossy(&csv).to_string()
        }
    };

    Ok(detailed_classes)
}

fn output_method_signatures(
    class_name: &str,
    methods: &[PythonMethod],
    format: OutFormat,
) -> Result<String> {
    let method_signatures = match format {
        OutFormat::Json => {
            let signatures: Vec<_> = methods.iter().map(|m| m.signature.clone()).collect();
            serde_json::to_string_pretty(&signatures)?
        }
        OutFormat::Csv => {
            let mut csv = Vec::new();
            writeln!(csv, "Signature")?;
            for method in methods {
                writeln!(csv, "\"{}\"", method.signature)?;
            }
            String::from_utf8_lossy(&csv).to_string()
        }
        OutFormat::Markdown => {
            let mut md = Vec::new();
            writeln!(md, "# `{class_name}` Method Signatures\n")?;
            for method in methods {
                writeln!(md, "```python\n{}\n```", method.signature)?;
                
                let Some(docstring) = &method.docstring else { continue };
                if docstring.is_empty() { continue; }
                
                let doc_preview = docstring.lines().next().unwrap_or("");
                if doc_preview.is_empty() { continue; }
                
                writeln!(md, "*{doc_preview}*\n")?;
            }
            String::from_utf8_lossy(&md).to_string()
        }
    };

    Ok(method_signatures)
}

fn output_class_with_docs(
    class: &PythonClass,
    methods: &[PythonMethod],
    format: OutFormat,
) -> Result<String> {
    let output = match format {
        OutFormat::Json => serde_json::to_string_pretty(&serde_json::json!({
            "class": {
                "name": class.name,
                "file": class.file_path,
                "line": class.line_number,
                "inherits": class.inherits,
                "docstring": class.docstring,
                "methods": methods,
            }
        }))?,
        OutFormat::Markdown => {
            let mut md = Vec::new();
            writeln!(md, "# Class: `{}`\n", class.name)?;
            writeln!(md, "**File**: `{}`", class.file_path)?;
            writeln!(md, "**Line**: {}", class.line_number.unwrap_or(0))?;
            if !class.inherits.is_empty() {
                writeln!(md, "**Inherits**: {}", class.inherits.join(", "))?;
            }
            writeln!(md)?;

            if let Some(docstring) = &class.docstring {
                if !docstring.is_empty() {
                    writeln!(md, "## Class Documentation\n")?;
                    writeln!(md, "{docstring}\n")?;
                }
            }

            if !methods.is_empty() {
                writeln!(md, "## Methods\n")?;
                for method in methods {
                    writeln!(md, "### `{}`\n", method.signature)?;
                    if method.is_async {
                        writeln!(md, "**Async**: Yes\n")?;
                    }
                    if method.is_static {
                        writeln!(md, "**Static**: Yes\n")?;
                    }
                    if !method.parameters.is_empty() {
                        writeln!(md, "**Parameters**:\n")?;
                        for param in &method.parameters {
                            writeln!(md, "- `{}`: `{}`", param.name, param.type_hint)?;
                        }
                        writeln!(md)?;
                    }
                    if !method.returns.is_empty() && method.returns != "Any" {
                        writeln!(md, "**Returns**: `{}`\n", method.returns)?;
                    }
                    if let Some(docstring) = &method.docstring {
                        if !docstring.is_empty() {
                            writeln!(md, "**Documentation**:\n")?;
                            writeln!(md, "{docstring}\n")?;
                        }
                    }
                }
            }
            String::from_utf8_lossy(&md).to_string()
        }
        OutFormat::Csv => {
            let mut csv = Vec::new();
            writeln!(csv, "Type,Name,Signature,Documentation")?;
            writeln!(
                csv,
                "Class,\"{}\",\"{}\",\"{}\"",
                class.name,
                format!("class {}({})", class.name, class.inherits.join(", ")),
                class
                    .docstring
                    .as_deref()
                    .unwrap_or("")
                    .replace('"', "\"\"")
            )?;
            for method in methods {
                writeln!(
                    csv,
                    "Method,\"{}\",\"{}\",\"{}\"",
                    method.name,
                    method.signature,
                    method
                        .docstring
                        .as_deref()
                        .unwrap_or("")
                        .replace('"', "\"\"")
                )?;
            }
            String::from_utf8_lossy(&csv).to_string()
        }
    };

    Ok(output)
}

fn output_class_methods(
    class_name: &str,
    methods: &[PythonMethod],
    format: OutFormat,
) -> Result<String> {
    let output = match format {
        OutFormat::Json => serde_json::to_string_pretty(methods)?,
        OutFormat::Csv => {
            let mut csv = Vec::new();
            writeln!(csv, "Class,Method,Signature,Parameters,Async,Static")?;
            for method in methods {
                let params: Vec<String> = method
                    .parameters
                    .iter()
                    .map(|p| format!("{}: {}", p.name, p.type_hint))
                    .collect();
                writeln!(
                    csv,
                    "\"{class_name}\",\"{}\",\"{}\",\"{}\",{},{}",
                    method.name,
                    method.signature,
                    params.join("; "),
                    method.is_async,
                    method.is_static
                )?;
            }
            String::from_utf8_lossy(&csv).to_string()
        }
        OutFormat::Markdown => {
            let mut md = Vec::new();
            writeln!(md, "# `{class_name}` Methods\n")?;
            writeln!(md, "| Method | Signature | Async | Static |")?;
            writeln!(md, "|--------|-----------|-------|--------|")?;
            for method in methods {
                writeln!(
                    md,
                    "| `{}` | `{}` | {} | {} |",
                    method.name,
                    method.signature.replace("|", "\\|"),
                    if method.is_async { "✓" } else { "" },
                    if method.is_static { "✓" } else { "" }
                )?;
            }
            String::from_utf8_lossy(&md).to_string()
        }
    };

    Ok(output)
}

fn output_functions(functions: &[PythonFunction], format: OutFormat) -> Result<String> {
    let output = match format {
        OutFormat::Json => serde_json::to_string_pretty(functions)?,
        OutFormat::Csv => {
            let mut csv = Vec::new();
            writeln!(csv, "Function,File,Signature,Parameters,Async")?;
            for func in functions {
                let params: Vec<String> = func
                    .parameters
                    .iter()
                    .map(|p| format!("{}: {}", p.name, p.type_hint))
                    .collect();
                writeln!(
                    csv,
                    "\"{}\",\"{}\",\"{}\",\"{}\",{}",
                    func.name,
                    func.file_path,
                    func.signature,
                    params.join("; "),
                    func.is_async
                )?;
            }
            String::from_utf8_lossy(&csv).to_string()
        }
        OutFormat::Markdown => {
            let mut md = Vec::new();
            writeln!(md, "# Functions\n")?;
            writeln!(md, "| Function | File | Signature | Async |")?;
            writeln!(md, "|----------|------|-----------|-------|")?;
            for func in functions {
                writeln!(
                    md,
                    "| `{}` | `{}` | `{}` | {} |",
                    func.name,
                    func.file_path,
                    func.signature.replace("|", "\\|"),
                    if func.is_async { "✓" } else { "" }
                )?;
            }
            String::from_utf8_lossy(&md).to_string()
        }
    };

    Ok(output)
}

fn generate_simple_stats(api: &PythonApi) -> Result<String> {
    let mut output = Vec::new();

    writeln!(output, "# iTerm2 Python API Stats\n")?;
    writeln!(
        output,
        "Generated on: {}\n",
        api.metadata.extraction_timestamp
    )?;

    // Overall statistics
    writeln!(output, "## Overall Statistics\n")?;
    writeln!(output, "- **Total Files**: {}", api.metadata.total_files)?;
    writeln!(
        output,
        "- **Total Classes**: {}",
        api.metadata.total_classes
    )?;
    writeln!(
        output,
        "- **Total Functions**: {}",
        api.metadata.total_functions
    )?;
    writeln!(output, "- **Total Enums**: {}", api.metadata.total_enums)?;
    writeln!(output)?;

    // Key classes analysis
    let key_classes = ["App", "Window", "Tab", "Session"];
    writeln!(output, "## Key Classes Analysis\n")?;

    for class_name in &key_classes {
        if let Some(class) = api
            .classes
            .iter()
            .find(|c| c.name == *class_name && !c.file_path.contains("mainmenu.py"))
        {
            writeln!(output, "### `{}`\n", class.name)?;
            writeln!(output, "- **Total Methods**: {}", class.methods.len())?;
            writeln!(output, "- **File**: `{}`", class.file_path)?;

            // Method analysis
            let async_methods = class.methods.iter().filter(|m| m.is_async).count();
            let static_methods = class.methods.iter().filter(|m| m.is_static).count();

            writeln!(output, "- **Async Methods**: {async_methods}")?;
            writeln!(output, "- **Static Methods**: {static_methods}")?;

            // Sample methods
            if !class.methods.is_empty() {
                writeln!(output, "- **Sample Methods**:")?;
                for method in class.methods.iter().take(5) {
                    writeln!(output, "  - `{}`", method.signature)?;
                }
            }
            writeln!(output)?;
        }
    }

    Ok(String::from_utf8_lossy(&output).to_string())
}

fn generate_detailed_stats(api: &PythonApi) -> Result<String> {
    let mut output = Vec::new();

    writeln!(output, "# iTerm2 Python API Detailed Stats\n")?;
    writeln!(
        output,
        "Generated on: {}\n",
        api.metadata.extraction_timestamp
    )?;

    // Overall statistics
    writeln!(output, "## Overall Statistics\n")?;
    writeln!(output, "- **Total Files**: {}", api.metadata.total_files)?;
    writeln!(
        output,
        "- **Total Classes**: {}",
        api.metadata.total_classes
    )?;
    writeln!(
        output,
        "- **Total Functions**: {}",
        api.metadata.total_functions
    )?;
    writeln!(output, "- **Total Enums**: {}", api.metadata.total_enums)?;
    writeln!(output)?;

    // Key classes analysis
    let key_classes = ["App", "Window", "Tab", "Session"];
    writeln!(output, "## Key Classes Analysis\n")?;

    for class_name in &key_classes {
        if let Some(class) = api
            .classes
            .iter()
            .find(|c| c.name == *class_name && !c.file_path.contains("mainmenu.py"))
        {
            writeln!(output, "### `{}`\n", class.name)?;
            writeln!(output, "- **Total Methods**: {}", class.methods.len())?;
            writeln!(output, "- **File**: `{}`", class.file_path)?;

            // Method analysis
            let async_methods = class.methods.iter().filter(|m| m.is_async).count();
            let static_methods = class.methods.iter().filter(|m| m.is_static).count();
            let methods_with_params = class
                .methods
                .iter()
                .filter(|m| !m.parameters.is_empty())
                .count();

            writeln!(output, "- **Async Methods**: {async_methods}")?;
            writeln!(output, "- **Static Methods**: {static_methods}")?;
            writeln!(
                output,
                "- **Methods with Parameters**: {methods_with_params}"
            )?;

            // Method categorization
            writeln!(output, "- **Method Categories**:")?;
            let method_categories = categorize_methods(&class.methods);
            for (category, count) in method_categories {
                writeln!(output, "  - {category}: {count}")?;
            }

            // Sample methods
            if !class.methods.is_empty() {
                writeln!(output, "- **Sample Methods**:")?;
                for method in class.methods.iter().take(5) {
                    writeln!(output, "  - `{}`", method.signature)?;
                }
            }
            writeln!(output)?;
        }
    }

    // Parameter frequency analysis
    writeln!(output, "## Parameter Frequency Analysis\n")?;
    let mut param_counts = std::collections::HashMap::new();

    for class in &api.classes {
        for method in &class.methods {
            for param in &method.parameters {
                *param_counts.entry(param.name.clone()).or_insert(0) += 1;
            }
        }
    }

    for func in &api.functions {
        for param in &func.parameters {
            *param_counts.entry(param.name.clone()).or_insert(0) += 1;
        }
    }

    let mut sorted_params: Vec<_> = param_counts.into_iter().collect();
    sorted_params.sort_by(|a, b| b.1.cmp(&a.1));

    writeln!(output, "| Parameter | Count |")?;
    writeln!(output, "|-----------|-------|")?;
    for (param, count) in sorted_params.iter().take(15) {
        writeln!(output, "| `{param}` | {count} |")?;
    }

    // Type analysis
    writeln!(output, "\n## Type Hint Analysis\n")?;
    let mut type_counts = std::collections::HashMap::new();

    for class in &api.classes {
        for method in &class.methods {
            for param in &method.parameters {
                *type_counts.entry(param.type_hint.clone()).or_insert(0) += 1;
            }
        }
    }

    for func in &api.functions {
        for param in &func.parameters {
            *type_counts.entry(param.type_hint.clone()).or_insert(0) += 1;
        }
    }

    let mut sorted_types: Vec<_> = type_counts.into_iter().collect();
    sorted_types.sort_by(|a, b| b.1.cmp(&a.1));

    writeln!(output, "| Type | Count |")?;
    writeln!(output, "|------|-------|")?;
    for (type_hint, count) in sorted_types.iter().take(15) {
        writeln!(output, "`{type_hint}` | {count} |")?;
    }

    // API coverage matrix
    writeln!(output, "\n## API Coverage Matrix\n")?;
    writeln!(output, "| Class | Methods | Properties | Status |")?;
    writeln!(output, "|-------|---------|------------|--------|")?;

    for class_name in &key_classes {
        if let Some(class) = api
            .classes
            .iter()
            .find(|c| c.name == *class_name && !c.file_path.contains("mainmenu.py"))
        {
            let status = if class.methods.len() > 10 {
                "✅ Well-covered"
            } else if class.methods.len() > 5 {
                "⚠️  Partially covered"
            } else {
                "❌ Needs investigation"
            };
            writeln!(
                output,
                "| `{}` | {} | {} | {status} |",
                class.name,
                class.methods.len(),
                class.properties.len()
            )?;
        }
    }

    Ok(String::from_utf8_lossy(&output).to_string())
}

fn extract_enum_values_from_class(_class: &PythonClass) -> Vec<EnumValue> {
    // For now, return empty vector as enum value extraction requires parsing the class body
    // This would need to be implemented by reading the file and parsing the class content
    Vec::new()
}

fn extract_default_value_from_param(param: &Node, source_code: &str) -> Option<String> {
    // Look for default value in parameter node
    let mut parent = param.parent();
    while let Some(current) = parent {
        if current.kind() != "parameters" && current.kind() != "default_parameter" {
            parent = current.parent();
            continue;
        }

        // Look for default value expression
        for i in 0..current.child_count() {
            let Some(child) = current.child(i) else { continue };
            
            let valid_kind = child.kind() == "string"
                || child.kind() == "integer"
                || child.kind() == "float"
                || child.kind() == "true"
                || child.kind() == "false";
            
            if valid_kind {
                return Some(source_code[child.start_byte()..child.end_byte()].to_string());
            }
        }
        parent = current.parent();
    }
    None
}

fn extract_decorator_name(decorator_node: &Node, source_code: &str) -> String {
    // Extract the actual decorator name from the source code
    for i in 0..decorator_node.child_count() {
        if let Some(child) = decorator_node.child(i) {
            if child.kind() == "identifier" {
                return source_code[child.start_byte()..child.end_byte()].to_string();
            }
        }
    }
    "unknown".to_string()
}

fn categorize_methods(methods: &[PythonMethod]) -> Vec<(String, usize)> {
    let mut categories = std::collections::HashMap::new();

    for method in methods {
        let category = if method.name.starts_with("get_") || method.name.starts_with("is_") {
            "Getter"
        } else if method.name.starts_with("set_") {
            "Setter"
        } else if method.name.contains("create") || method.name.contains("new") {
            "Factory"
        } else if method.name.contains("async") && method.is_async {
            "Async Operation"
        } else if method.is_static {
            "Static Utility"
        } else {
            "General Method"
        };

        *categories.entry(category.to_string()).or_insert(0) += 1;
    }

    let mut result: Vec<_> = categories.into_iter().collect();
    result.sort_by(|a, b| b.1.cmp(&a.1));
    result
}
