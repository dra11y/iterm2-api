#![allow(unused)]

use anyhow::{Result, anyhow};
use chrono::{self, Utc};
use clap::{Parser, Subcommand, ValueEnum};
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    fs::read_dir,
    io::Write,
    path::{Path, PathBuf},
};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};
use tracing_subscriber::fmt;
use tree_parser::{
    CodeConstruct, Language, ParsedFile, parse_file, search_by_node_type, search_by_query,
};

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
    docstring: String,
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
    docstring: String,
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
    docstring: String,
    is_readonly: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PythonEnum {
    name: String,
    file_path: String,
    docstring: String,
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
    docstring: String,
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
            execute_list_command(&api, class, detailed, cli.format)?;
        }
        Commands::Query {
            class,
            method,
            parameter,
            signatures,
            docs,
        } => {
            execute_query_command(
                &api, &class, method, parameter, signatures, docs, cli.format,
            )?;
        }
        Commands::Functions {
            name,
            parameter,
            async_only,
        } => {
            execute_functions_command(&api, name, parameter, async_only, cli.format)?;
        }
        Commands::Stats { detailed } => {
            generate_stats(&api, detailed)?;
        }
        Commands::Extract { full } => {
            extract_api_structure(&api, full)?;
        }
    }

    Ok(())
}

async fn parse_python_api(source_path: &str) -> Result<PythonApi> {
    let source_dir = Path::new(source_path);
    if !source_dir.exists() {
        return Err(anyhow!("Source directory does not exist: {}", source_path));
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

            if path.is_dir() {
                // Recursively search subdirectories
                collect_python_files(&path, parse_futures, total_files, source_dir)?;
            } else if path.extension().and_then(|s| s.to_str()) == Some("py") {
                if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                    if !file_name.starts_with('_') || file_name == "__init__.py" {
                        *total_files += 1;
                        let file_path = path.clone();
                        let file_name_clone = file_name.to_string();
                        info!("Parsing file: {}", file_path.display());
                        let source_dir_clone = source_dir.to_path_buf();
                        parse_futures.push(tokio::spawn(async move {
                            let file_start = std::time::Instant::now();
                            let result =
                                match parse_python_file(&file_path, &source_dir_clone).await {
                                    Ok(file_api) => Some(file_api),
                                    Err(e) => {
                                        debug!("Failed to parse {}: {}", file_name_clone, e);
                                        None
                                    }
                                };
                            let file_duration = file_start.elapsed();
                            if file_duration.as_millis() > 100 {
                                debug!(
                                    "Slow file parse: {} took {:?}",
                                    file_name_clone, file_duration
                                );
                            }
                            result
                        }));
                    }
                }
            }
        }
        Ok(())
    }

    collect_python_files(source_dir, &mut parse_futures, &mut total_files, source_dir)?;

    // Wait for all parsing to complete
    let start_time = std::time::Instant::now();
    debug!("Waiting for parsing...");
    let results = join_all(parse_futures).await;
    let join_duration = start_time.elapsed();
    debug!("Parsing complete! join_all took: {:?}", join_duration);

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
                    debug!("Task failed: {}", join_error);
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

async fn parse_python_file(file_path: &Path, source_dir: &Path) -> Result<FileApi> {
    debug!("parse_python_file: {}", file_path.display());
    let file_str = file_path.to_string_lossy().to_string();

    // Try to load from cache first
    if let Ok(cached_data) = load_from_cache(file_path, source_dir) {
        info!("parse_python_file CACHE HIT: {}", file_path.display());
        return Ok(cached_data);
    }

    // Parse the Python file using tree-parser
    let parsed_file = match parse_file(&file_str, Language::Python).await {
        Ok(parsed) => parsed,
        Err(e) => {
            debug!("Failed to parse file {}: {}", file_path.display(), e);
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
    let class_constructs = search_by_node_type(&parsed_file, "class_definition", None);
    for class_construct in class_constructs {
        if let Some(_class_name) = &class_construct.name {
            match parse_class_definition(&parsed_file, &class_construct, file_path) {
                Ok(class) => classes.push(class),
                Err(e) => debug!(
                    "Failed to parse class definition in {}: {}",
                    file_path.display(),
                    e
                ),
            }
        }
    }

    // Find all function definitions (not inside classes)
    let function_constructs = search_by_node_type(&parsed_file, "function_definition", None);
    for func_construct in function_constructs {
        if let Some(_func_name) = &func_construct.name {
            // Check if this function is not inside a class
            match is_inside_class(&parsed_file, &func_construct) {
                Ok(false) => {
                    match parse_function_definition(&parsed_file, &func_construct, file_path) {
                        Ok(func) => functions.push(func),
                        Err(e) => debug!(
                            "Failed to parse function definition in {}: {}",
                            file_path.display(),
                            e
                        ),
                    }
                }
                Ok(true) => {} // Function is inside a class, skip
                Err(e) => debug!(
                    "Failed to check if function is inside class in {}: {}",
                    file_path.display(),
                    e
                ),
            }
        }
    }

    // Find all enum definitions (Python enums are typically classes inheriting from Enum)
    // First check if Enum is imported in this file
    match inherits_from_enum(&parsed_file) {
        Ok(true) => {
            let all_class_constructs = search_by_node_type(&parsed_file, "class_definition", None);
            for class_construct in all_class_constructs {
                if let Some(_class_name) = &class_construct.name {
                    // Check if it inherits from Enum
                    match find_superclasses(&parsed_file, &class_construct) {
                        Ok(Some(superclasses)) => {
                            if superclasses.iter().any(|superclass| {
                                superclass == "Enum" || superclass.ends_with("Enum")
                            }) {
                                match parse_enum_definition(
                                    &parsed_file,
                                    &class_construct,
                                    file_path,
                                ) {
                                    Ok(enum_def) => enums.push(enum_def),
                                    Err(e) => debug!(
                                        "Failed to parse enum definition in {}: {}",
                                        file_path.display(),
                                        e
                                    ),
                                }
                            }
                        }
                        Ok(None) => {} // No superclasses
                        Err(e) => debug!(
                            "Failed to find superclasses in {}: {}",
                            file_path.display(),
                            e
                        ),
                    }
                }
            }
        }
        Ok(false) => {} // No Enum import
        Err(e) => debug!(
            "Failed to check for Enum import in {}: {}",
            file_path.display(),
            e
        ),
    }

    let result = FileApi {
        classes,
        enums,
        functions,
    };

    // Save to cache for future runs
    if let Err(e) = save_to_cache(file_path, source_dir, &result) {
        debug!("Failed to cache {}: {}", file_path.display(), e);
    }

    Ok(result)
}

fn parse_class_definition(
    parsed_file: &ParsedFile,
    class_construct: &CodeConstruct,
    file_path: &Path,
) -> Result<PythonClass> {
    let mut methods = Vec::new();
    let mut properties = Vec::new();
    let mut inherits = Vec::new();

    // Extract inheritance
    match find_superclasses(parsed_file, class_construct) {
        Ok(Some(superclasses)) => inherits = superclasses,
        Ok(None) => {} // No superclasses
        Err(e) => debug!("Failed to find superclasses for class: {}", e),
    }

    // Find methods inside this class - simpler approach
    let all_function_constructs = search_by_node_type(parsed_file, "function_definition", None);
    for func_construct in all_function_constructs {
        if is_within_construct(parsed_file, &func_construct, class_construct) {
            if let Some(_method_name) = &func_construct.name {
                match parse_method_definition(parsed_file, &func_construct) {
                    Ok(method) => methods.push(method),
                    Err(e) => debug!("Failed to parse method definition: {}", e),
                }
            }
        }
    }

    // Find properties (decorated with @property) - simpler approach
    let all_decorated_constructs = search_by_node_type(parsed_file, "decorated_definition", None);
    for decorated_construct in all_decorated_constructs {
        if is_within_construct(parsed_file, &decorated_construct, class_construct) {
            match is_property_decorator(parsed_file, &decorated_construct) {
                Ok(true) => {
                    if let Some(_property_name) = &decorated_construct.name {
                        match parse_property_definition(parsed_file, &decorated_construct) {
                            Ok(property) => properties.push(property),
                            Err(e) => debug!("Failed to parse property definition: {}", e),
                        }
                    }
                }
                Ok(false) => {} // Not a property decorator
                Err(e) => debug!("Failed to check if decorated definition is property: {}", e),
            }
        }
    }

    let is_exception = inherits.contains(&"Exception".to_string());
    let is_abstract =
        inherits.contains(&"ABC".to_string()) || inherits.iter().any(|s| s.contains("Abstract"));

    Ok(PythonClass {
        name: class_construct.name.clone().unwrap_or_default(),
        file_path: file_path.to_string_lossy().to_string(),
        docstring: extract_docstring(parsed_file, class_construct),
        methods,
        properties,
        inherits,
        line_number: Some(class_construct.start_line as u32),
        is_exception,
        is_abstract,
    })
}

fn parse_method_definition(
    parsed_file: &ParsedFile,
    method_construct: &CodeConstruct,
) -> Result<PythonMethod> {
    let parameters = extract_parameters(parsed_file, method_construct);
    let is_async = is_async_function(parsed_file, method_construct);
    let is_static = is_static_method(parsed_file, method_construct);
    let decorators = extract_decorators(parsed_file, method_construct);

    Ok(PythonMethod {
        name: method_construct.name.clone().unwrap_or_default(),
        signature: extract_signature(parsed_file, method_construct),
        docstring: extract_docstring(parsed_file, method_construct),
        parameters,
        returns: extract_return_type(parsed_file, method_construct),
        is_async,
        is_static,
        decorators,
    })
}

fn parse_function_definition(
    parsed_file: &ParsedFile,
    func_construct: &CodeConstruct,
    file_path: &Path,
) -> Result<PythonFunction> {
    let parameters = extract_parameters(parsed_file, func_construct);
    let is_async = is_async_function(parsed_file, func_construct);

    Ok(PythonFunction {
        name: func_construct.name.clone().unwrap_or_default(),
        file_path: file_path.to_string_lossy().to_string(),
        signature: extract_signature(parsed_file, func_construct),
        docstring: extract_docstring(parsed_file, func_construct),
        parameters,
        returns: extract_return_type(parsed_file, func_construct),
        is_async,
    })
}

fn parse_enum_definition(
    parsed_file: &ParsedFile,
    enum_construct: &CodeConstruct,
    file_path: &Path,
) -> Result<PythonEnum> {
    let values = match extract_enum_values(parsed_file, enum_construct) {
        Ok(values) => values,
        Err(e) => {
            debug!("Failed to extract enum values: {}", e);
            Vec::new()
        }
    };

    Ok(PythonEnum {
        name: enum_construct.name.clone().unwrap_or_default(),
        file_path: file_path.to_string_lossy().to_string(),
        docstring: extract_docstring(parsed_file, enum_construct),
        values,
    })
}

fn parse_property_definition(
    parsed_file: &ParsedFile,
    property_construct: &CodeConstruct,
) -> Result<PythonProperty> {
    Ok(PythonProperty {
        name: property_construct.name.clone().unwrap_or_default(),
        type_hint: extract_return_type(parsed_file, property_construct),
        docstring: extract_docstring(parsed_file, property_construct),
        is_readonly: !has_setter(parsed_file, property_construct),
    })
}

// Helper functions for extraction
fn extract_docstring(parsed_file: &ParsedFile, construct: &CodeConstruct) -> String {
    // Look for string literals within the construct
    let string_literals = search_by_node_type(parsed_file, "string_literal", None);

    for string_literal in string_literals {
        if is_within_construct(parsed_file, &string_literal, construct) {
            // Check if this is likely a docstring (first string literal in construct)
            if !string_literal.source_code.is_empty() {
                // Check if it's near the beginning of the construct
                if (string_literal.start_byte - construct.start_byte) < 300 {
                    // Clean up the string literal (remove quotes)
                    let mut content = string_literal.source_code.clone();
                    if content.starts_with('"') && content.ends_with('"') && content.len() > 2 {
                        content = content[1..content.len() - 1].to_string();
                    } else if content.starts_with('\'')
                        && content.ends_with('\'')
                        && content.len() > 2
                    {
                        content = content[1..content.len() - 1].to_string();
                    } else if content.starts_with("\"\"\"")
                        && content.ends_with("\"\"\"")
                        && content.len() > 6
                    {
                        content = content[3..content.len() - 3].to_string();
                    } else if content.starts_with("'''")
                        && content.ends_with("'''")
                        && content.len() > 6
                    {
                        content = content[3..content.len() - 3].to_string();
                    }

                    // Only return if it's a reasonable docstring (not empty)
                    if !content.trim().is_empty() {
                        return content;
                    }
                }
            }
        }
    }

    String::new()
}

fn extract_parameters(_parsed_file: &ParsedFile, construct: &CodeConstruct) -> Vec<Parameter> {
    // Debug: Log what we're looking for
    debug!("Extracting parameters for function: {:?}", construct.name);
    debug!(
        "Function construct spans bytes {} to {}",
        construct.start_byte, construct.end_byte
    );

    // Check if the tree-parser already extracted parameters in the metadata
    debug!("Metadata parameters: {:?}", construct.metadata.parameters);

    if !construct.metadata.parameters.is_empty() {
        debug!("Using parameters from metadata");
        // Convert tree-parser Parameter to our Parameter type
        return construct
            .metadata
            .parameters
            .iter()
            .map(|p| Parameter {
                name: p.name.clone(),
                type_hint: p.param_type.clone().unwrap_or_else(|| "Any".to_string()),
                default_value: p.default_value.clone(),
            })
            .collect();
    }

    // If metadata doesn't have parameters, try a more direct approach
    // Look for parameters node within the function definition using tree-sitter queries
    let mut parameters = Vec::new();

    // Debug: Check children of this function construct
    debug!(
        "Function construct has {} children",
        construct.children.len()
    );
    for (i, child) in construct.children.iter().enumerate() {
        debug!("Child {}: {} ({:?})", i, child.node_type, child.name);
    }

    // Try to find parameters using a tree-sitter query
    let _parameters_query = r#"
    (function_definition
      parameters: (parameters
        (identifier) @param_name
        (type) @param_type?
      )?
    )
    "#;

    // For now, let's try a simpler approach: parse the source code directly
    // Extract the function signature from the source code
    let source = &construct.source_code;
    debug!("Function source code: {}", source);

    // Look for the parameter list between parentheses
    if let Some(start) = source.find('(') {
        if let Some(end) = source.find(')') {
            let param_section = &source[start + 1..end];
            debug!("Parameter section: {}", param_section);

            // Split by commas and parse each parameter
            for param_str in param_section.split(',') {
                let param_str = param_str.trim();
                if !param_str.is_empty() {
                    // Skip 'self' and 'cls' parameters
                    if param_str == "self" || param_str == "cls" {
                        continue;
                    }

                    // Split parameter name and type (if present)
                    let parts: Vec<&str> = param_str.split(':').collect();
                    let name = parts[0].trim();

                    if !name.is_empty() {
                        let type_hint = if parts.len() > 1 {
                            parts[1].trim().to_string()
                        } else {
                            "Any".to_string()
                        };

                        // Check for default values
                        let default_value = if let Some(eq_pos) = name.find('=') {
                            Some(name[eq_pos + 1..].trim().to_string())
                        } else {
                            None
                        };

                        let final_name = if let Some(eq_pos) = name.find('=') {
                            name[..eq_pos].trim()
                        } else {
                            name
                        };

                        if !final_name.is_empty() {
                            debug!("Adding parameter: {} with type: {}", final_name, type_hint);
                            parameters.push(Parameter {
                                name: final_name.to_string(),
                                type_hint,
                                default_value,
                            });
                        }
                    }
                }
            }
        }
    }

    debug!("Final parameters: {:?}", parameters);
    parameters
}

fn extract_signature(parsed_file: &ParsedFile, construct: &CodeConstruct) -> String {
    let name = construct.name.clone().unwrap_or_default();
    let parameters = extract_parameters(parsed_file, construct);

    let param_str = parameters
        .iter()
        .map(|p| {
            if let Some(default) = &p.default_value {
                format!("{}: {} = {}", p.name, p.type_hint, default)
            } else if !p.type_hint.is_empty() {
                format!("{}: {}", p.name, p.type_hint)
            } else {
                p.name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(", ");

    format!("{}({})", name, param_str)
}

fn extract_return_type(parsed_file: &ParsedFile, construct: &CodeConstruct) -> String {
    // Look for return type annotation
    let return_query = format!(
        "(function_definition
          return_type: (type) @return_type
        )"
    );

    if let Ok(return_constructs) = search_by_query(parsed_file, &return_query) {
        for return_construct in return_constructs {
            if is_within_construct(parsed_file, &return_construct, construct) {
                if !return_construct.source_code.is_empty() {
                    return return_construct.source_code.clone();
                }
            }
        }
    }

    "Any".to_string()
}

fn is_async_function(parsed_file: &ParsedFile, construct: &CodeConstruct) -> bool {
    // Look for async keyword nodes within the function definition
    let async_nodes = search_by_node_type(parsed_file, "async", None);

    for async_node in async_nodes {
        if is_within_construct(parsed_file, &async_node, construct) {
            return true;
        }
    }

    false
}

fn is_static_method(parsed_file: &ParsedFile, construct: &CodeConstruct) -> bool {
    // Check if the method has @staticmethod decorator
    let decorators = extract_decorators(parsed_file, construct);
    decorators.contains(&"staticmethod".to_string())
}

fn extract_decorators(parsed_file: &ParsedFile, construct: &CodeConstruct) -> Vec<String> {
    let mut decorators = Vec::new();

    // Look for decorator nodes
    let decorator_nodes = search_by_node_type(parsed_file, "decorator", None);

    for decorator_node in decorator_nodes {
        if is_within_construct(parsed_file, &decorator_node, construct) {
            // Look for identifiers within this decorator
            let decorator_identifiers = search_by_node_type(parsed_file, "identifier", None);
            for identifier in decorator_identifiers {
                if is_within_construct(parsed_file, &identifier, &decorator_node) {
                    if let Some(name) = &identifier.name {
                        decorators.push(name.clone());
                    }
                }
            }
        }
    }

    decorators
}

fn is_inside_class(parsed_file: &ParsedFile, construct: &CodeConstruct) -> Result<bool> {
    // Look for parent class_definition
    let class_query = format!(
        "(class_definition
          body: (block
            (function_definition) @nested_func
          )
        )"
    );

    match search_by_query(parsed_file, &class_query) {
        Ok(class_constructs) => {
            for class_construct in class_constructs {
                if is_within_construct(parsed_file, construct, &class_construct) {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Err(e) => Err(anyhow!("Failed to search for class definitions: {}", e)),
    }
}

fn find_superclasses(
    parsed_file: &ParsedFile,
    construct: &CodeConstruct,
) -> Result<Option<Vec<String>>> {
    let mut superclasses = Vec::new();

    // Look for argument_list in class definition
    let superclass_query = format!(
        "(class_definition
          superclasses: (argument_list
            (identifier) @superclass
          )
        )"
    );

    match search_by_query(parsed_file, &superclass_query) {
        Ok(superclass_constructs) => {
            for superclass_construct in superclass_constructs {
                if is_within_construct(parsed_file, &superclass_construct, construct) {
                    if let Some(name) = &superclass_construct.name {
                        superclasses.push(name.clone());
                    }
                }
            }
        }
        Err(e) => return Err(anyhow!("Failed to search for superclasses: {}", e)),
    }

    if superclasses.is_empty() {
        Ok(None)
    } else {
        Ok(Some(superclasses))
    }
}

fn inherits_from_enum(parsed_file: &ParsedFile) -> Result<bool> {
    // Look for import statements that import Enum
    let import_nodes = search_by_node_type(parsed_file, "import_statement", None);

    for import_node in import_nodes {
        // Look for identifiers within import statements
        let import_identifiers = search_by_node_type(parsed_file, "identifier", None);

        for identifier in import_identifiers {
            if is_within_construct(parsed_file, &identifier, &import_node) {
                if let Some(name) = &identifier.name {
                    if name == "Enum" {
                        return Ok(true);
                    }
                }
            }
        }
    }

    // Also check for from imports
    let import_from_nodes = search_by_node_type(parsed_file, "import_from_statement", None);

    for import_from_node in import_from_nodes {
        // Look for identifiers within import from statements
        let import_identifiers = search_by_node_type(parsed_file, "identifier", None);

        for identifier in import_identifiers {
            if is_within_construct(parsed_file, &identifier, &import_from_node) {
                if let Some(name) = &identifier.name {
                    if name == "Enum" {
                        return Ok(true);
                    }
                }
            }
        }
    }

    Ok(false)
}

fn is_property_decorator(parsed_file: &ParsedFile, construct: &CodeConstruct) -> Result<bool> {
    // Look for decorator nodes within the decorated definition
    let decorator_nodes = search_by_node_type(parsed_file, "decorator", None);

    for decorator_node in decorator_nodes {
        if is_within_construct(parsed_file, &decorator_node, construct) {
            // Look for identifiers within this decorator
            let decorator_identifiers = search_by_node_type(parsed_file, "identifier", None);

            for identifier in decorator_identifiers {
                if is_within_construct(parsed_file, &identifier, &decorator_node) {
                    if let Some(name) = &identifier.name {
                        if name == "property" {
                            return Ok(true);
                        }
                    }
                }
            }
        }
    }

    Ok(false)
}

fn has_setter(parsed_file: &ParsedFile, construct: &CodeConstruct) -> bool {
    // Look for setter method with same name as property
    if let Some(prop_name) = &construct.name {
        let setter_query = format!(
            "(decorated_definition
              decorator: (decorator
                (attribute
                  object: (identifier) @obj_name
                  attribute: (identifier) @attr_name
                )
              )
              definition: (function_definition
                name: (identifier) @setter_name
              )
            )"
        );

        if let Ok(setter_constructs) = search_by_query(parsed_file, &setter_query) {
            for setter_construct in setter_constructs {
                if let Some(setter_name) = &setter_construct.name {
                    if setter_name == prop_name {
                        return true;
                    }
                }
            }
        }
    }

    false
}

fn extract_enum_values(
    parsed_file: &ParsedFile,
    construct: &CodeConstruct,
) -> Result<Vec<EnumValue>> {
    let mut values = Vec::new();

    // Look for assignment statements within the enum class
    let enum_value_query = format!(
        "(class_definition
          body: (block
            (assignment
              left: (identifier) @enum_value_name
              right: (_) @enum_value
            )
          )
        )"
    );

    match search_by_query(parsed_file, &enum_value_query) {
        Ok(enum_value_constructs) => {
            for enum_value_construct in enum_value_constructs {
                if is_within_construct(parsed_file, &enum_value_construct, construct) {
                    if let Some(name) = &enum_value_construct.name {
                        let value = if enum_value_construct.source_code.is_empty() {
                            None
                        } else {
                            Some(enum_value_construct.source_code.clone())
                        };
                        values.push(EnumValue {
                            name: name.clone(),
                            value,
                        });
                    }
                }
            }
        }
        Err(e) => return Err(anyhow!("Failed to search for enum values: {}", e)),
    }

    Ok(values)
}

fn is_within_construct(
    _parsed_file: &ParsedFile,
    inner: &CodeConstruct,
    outer: &CodeConstruct,
) -> bool {
    // Check if inner construct is within outer construct using position information
    let (inner_start, inner_end, outer_start, outer_end) = (
        inner.start_byte,
        inner.end_byte,
        outer.start_byte,
        outer.end_byte,
    );

    inner_start >= outer_start && inner_end <= outer_end
}

// Command implementations
fn execute_list_command(
    api: &PythonApi,
    class_filter: Option<String>,
    detailed: bool,
    format: OutFormat,
) -> Result<()> {
    let mut filtered_classes = api.classes.clone();

    // Apply class filter if provided
    if let Some(filter) = &class_filter {
        filtered_classes.retain(|class| class.name.to_lowercase().contains(&filter.to_lowercase()));
    }

    // Sort classes by name
    filtered_classes.sort_by(|a, b| a.name.cmp(&b.name));

    if detailed {
        output_detailed_classes(&filtered_classes, format)?;
    } else {
        output_class_summary(&filtered_classes, format)?;
    }

    Ok(())
}

fn execute_query_command(
    api: &PythonApi,
    class_name: &str,
    method_filter: Option<String>,
    parameter_filter: Option<String>,
    signatures_only: bool,
    show_docs: bool,
    format: OutFormat,
) -> Result<()> {
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
        .ok_or_else(|| anyhow!("Class '{}' not found", class_name))?;

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

    if signatures_only {
        output_method_signatures(&class.name, &methods, format)?;
    } else if show_docs {
        output_class_with_docs(class, &methods, format)?;
    } else {
        output_class_methods(&class.name, &methods, format)?;
    }

    Ok(())
}

fn execute_functions_command(
    api: &PythonApi,
    name_filter: Option<String>,
    parameter_filter: Option<String>,
    async_only: bool,
    format: OutFormat,
) -> Result<()> {
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

    output_functions(&functions, format)?;
    Ok(())
}

fn generate_stats(api: &PythonApi, detailed: bool) -> Result<()> {
    let stats = if detailed {
        generate_detailed_stats(api)?
    } else {
        generate_simple_stats(api)?
    };

    println!("{stats}");
    Ok(())
}

fn extract_api_structure(api: &PythonApi, full: bool) -> Result<()> {
    let structure = if full {
        serde_json::to_string_pretty(api)?
    } else {
        // Extract only classes and methods for basic structure
        let simplified = PythonApi {
            classes: api.classes.clone(),
            enums: Vec::new(),     // Skip enums in basic mode
            functions: Vec::new(), // Skip functions in basic mode
            metadata: api.metadata.clone(),
        };
        serde_json::to_string_pretty(&simplified)?
    };

    println!("{structure}");
    Ok(())
}

// Output functions for different commands
fn output_class_summary(classes: &[PythonClass], format: OutFormat) -> Result<()> {
    let output = match format {
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

    println!("{output}");
    Ok(())
}

fn output_detailed_classes(classes: &[PythonClass], format: OutFormat) -> Result<()> {
    let output = match format {
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

                if !class.methods.is_empty() {
                    writeln!(md, "### Methods\n")?;
                    for method in &class.methods {
                        writeln!(md, "- `{}`", method.signature)?;
                        if !method.docstring.is_empty() {
                            let doc_preview = method.docstring.lines().next().unwrap_or("");
                            if !doc_preview.is_empty() {
                                writeln!(md, "  - *{}*", doc_preview)?;
                            }
                        }
                    }
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
                        method.docstring.replace("\"", "\"\"")
                    )?;
                }
            }
            String::from_utf8_lossy(&csv).to_string()
        }
    };

    println!("{output}");
    Ok(())
}

fn output_method_signatures(
    class_name: &str,
    methods: &[PythonMethod],
    format: OutFormat,
) -> Result<()> {
    let output = match format {
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
            writeln!(md, "# `{}` Method Signatures\n", class_name)?;
            for method in methods {
                writeln!(md, "```python\n{}\n```", method.signature)?;
                if !method.docstring.is_empty() {
                    let doc_preview = method.docstring.lines().next().unwrap_or("");
                    if !doc_preview.is_empty() {
                        writeln!(md, "*{}*\n", doc_preview)?;
                    }
                }
            }
            String::from_utf8_lossy(&md).to_string()
        }
    };

    println!("{output}");
    Ok(())
}

fn output_class_with_docs(
    class: &PythonClass,
    methods: &[PythonMethod],
    format: OutFormat,
) -> Result<()> {
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

            if !class.docstring.is_empty() {
                writeln!(md, "## Class Documentation\n")?;
                writeln!(md, "{}\n", class.docstring)?;
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
                    if !method.docstring.is_empty() {
                        writeln!(md, "**Documentation**:\n")?;
                        writeln!(md, "{}\n", method.docstring)?;
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
                class.docstring.replace("\"", "\"\"")
            )?;
            for method in methods {
                writeln!(
                    csv,
                    "Method,\"{}\",\"{}\",\"{}\"",
                    method.name,
                    method.signature,
                    method.docstring.replace("\"", "\"\"")
                )?;
            }
            String::from_utf8_lossy(&csv).to_string()
        }
    };

    println!("{output}");
    Ok(())
}

fn output_class_methods(
    class_name: &str,
    methods: &[PythonMethod],
    format: OutFormat,
) -> Result<()> {
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
                    "\"{}\",\"{}\",\"{}\",\"{}\",{},{}",
                    class_name,
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
            writeln!(md, "# `{}` Methods\n", class_name)?;
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

    println!("{output}");
    Ok(())
}

fn output_functions(functions: &[PythonFunction], format: OutFormat) -> Result<()> {
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

    println!("{output}");
    Ok(())
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

            writeln!(output, "- **Async Methods**: {}", async_methods)?;
            writeln!(output, "- **Static Methods**: {}", static_methods)?;

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

            writeln!(output, "- **Async Methods**: {}", async_methods)?;
            writeln!(output, "- **Static Methods**: {}", static_methods)?;
            writeln!(
                output,
                "- **Methods with Parameters**: {}",
                methods_with_params
            )?;

            // Method categorization
            writeln!(output, "- **Method Categories**:")?;
            let method_categories = categorize_methods(&class.methods);
            for (category, count) in method_categories {
                writeln!(output, "  - {}: {}", category, count)?;
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
        writeln!(output, "| `{}` | {} |", param, count)?;
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
        writeln!(output, "`{}` | {} |", type_hint, count)?;
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
                "| `{}` | {} | {} | {} |",
                class.name,
                class.methods.len(),
                class.properties.len(),
                status
            )?;
        }
    }

    Ok(String::from_utf8_lossy(&output).to_string())
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

// fn show_summary(api: &PythonApi) {
//     println!("📊 iTerm2 Python API Summary");
//     println!("═══════════════════════════");
//     println!("📁 Total Files: {}", api.metadata.total_files);
//     println!("🏗️  Total Classes: {}", api.metadata.total_classes);
//     println!("⚙️  Total Functions: {}", api.metadata.total_functions);
//     println!("🔢 Total Enums: {}", api.metadata.total_enums);
//     println!();

//     // Key classes
//     let key_classes = ["App", "Window", "Tab", "Session"];
//     println!("🎯 Key Classes:");
//     for class_name in &key_classes {
//         if let Some(class) = api.classes.iter().find(|c| c.name == *class_name && !c.file_path.contains("mainmenu.py")) {
//             println!("  • {}: {} methods", class.name, class.methods.len());
//         }
//     }
//     println!();

//     // Method statistics
//     let total_methods: usize = api.classes.iter().map(|c| c.methods.len()).sum();
//     let async_methods: usize = api
//         .classes
//         .iter()
//         .flat_map(|c| c.methods.iter())
//         .filter(|m| m.is_async)
//         .count();

//     println!("📈 Method Statistics:");
//     println!("  • Total Methods: {}", total_methods);
//     println!("  • Async Methods: {}", async_methods);
//     println!("  • Sync Methods: {}", total_methods - async_methods);
//     println!();

//     println!("💡 Use 'python-parser --help' to see available commands");
// }

// Cache functions
fn get_cache_path(file_path: &Path, source_dir: &Path) -> Result<PathBuf> {
    let cache_dir = Path::new(".cache");
    fs::create_dir_all(cache_dir)?;

    // Get relative path from source directory
    let relative_path = match file_path.strip_prefix(source_dir) {
        Ok(path) => path,
        Err(_) => Path::new(
            file_path
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("unknown")),
        ),
    };

    let cache_file_path = cache_dir.join(relative_path).with_extension("json");

    // Create parent directories if needed
    if let Some(parent) = cache_file_path.parent() {
        fs::create_dir_all(parent)?;
    }

    Ok(cache_file_path)
}

fn load_from_cache(file_path: &Path, source_dir: &Path) -> Result<FileApi> {
    let cache_path = get_cache_path(file_path, source_dir)?;

    if !cache_path.exists() {
        return Err(anyhow!("Cache file does not exist"));
    }

    let cached_content = fs::read_to_string(cache_path)?;
    let cached_data: FileApi = serde_json::from_str(&cached_content)?;

    Ok(cached_data)
}

fn save_to_cache(file_path: &Path, source_dir: &Path, data: &FileApi) -> Result<()> {
    let cache_path = get_cache_path(file_path, source_dir)?;

    debug!("Saving cache to: {:?}", cache_path);

    let json_content = serde_json::to_string_pretty(data)?;
    fs::write(cache_path, json_content)?;

    debug!("Cache saved successfully");

    Ok(())
}
