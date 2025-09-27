use anyhow::Result;
use chrono;
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use tracing::{info, warn};
use tracing_subscriber::fmt;

#[derive(Parser)]
#[command(name = "python-parser")]
#[command(about = "Extracts iTerm2 Python API structure from source code")]
struct Cli {
    /// Path to Python source directory
    #[arg(long)]
    source_path: String,
    
    /// Output JSON file path
    #[arg(long)]
    output: Option<String>,
    
    /// Query mode: filter classes and methods
    #[arg(long)]
    query: Option<String>,
    
    /// Filter by class name (comma-separated)
    #[arg(long)]
    class: Option<String>,
    
    /// Filter by method name pattern
    #[arg(long)]
    method: Option<String>,
    
    /// Filter by parameter name
    #[arg(long)]
    parameter: Option<String>,
    
    /// Export format (json, csv, markdown)
    #[arg(long, default_value = "json")]
    format: String,
    
    /// Generate progress report for PROGRESS.md
    #[arg(long)]
    progress: bool,
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
    fmt::init();
    
    let cli = Cli::parse();
    
    info!("Parsing Python API from: {}", cli.source_path);
    let api = parse_python_api(&cli.source_path).await?;
    
    // Handle different output modes
    if cli.progress {
        generate_progress_report(&api)?;
    } else if let Some(query) = &cli.query {
        execute_query(&api, query)?;
    } else if let Some(output_path) = &cli.output {
        // Full export
        match cli.format.as_str() {
            "json" => {
                let json = serde_json::to_string_pretty(&api)?;
                fs::write(output_path, json)?;
                info!("Extracted API data to: {}", output_path);
            }
            "csv" => {
                export_to_csv(&api, output_path)?;
                info!("Exported API data to CSV: {}", output_path);
            }
            "markdown" => {
                export_to_markdown(&api, output_path)?;
                info!("Exported API data to Markdown: {}", output_path);
            }
            _ => {
                return Err(anyhow::anyhow!("Unsupported format: {}", cli.format));
            }
        }
    } else {
        // Default: show summary
        show_summary(&api);
    }
    
    Ok(())
}

async fn parse_python_api(source_path: &str) -> Result<PythonApi> {
    let source_dir = Path::new(source_path);
    if !source_dir.exists() {
        return Err(anyhow::anyhow!("Source directory does not exist: {}", source_path));
    }
    
    let mut classes = Vec::new();
    let mut enums = Vec::new();
    let mut functions = Vec::new();
    let mut total_files = 0;
    
    // Parse all Python files in the directory recursively
    let mut parse_futures = Vec::new();
    
    fn collect_python_files(dir: &Path, parse_futures: &mut Vec<tokio::task::JoinHandle<Option<FileApi>>>, total_files: &mut usize) -> Result<()> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if path.is_dir() {
                // Recursively search subdirectories
                collect_python_files(&path, parse_futures, total_files)?;
            } else if path.extension().and_then(|s| s.to_str()) == Some("py") {
                if let Some(file_name) = path.file_name().and_then(|s| s.to_str()) {
                    if !file_name.starts_with('_') || file_name == "__init__.py" {
                        *total_files += 1;
                        let file_path = path.clone();
                        let file_name_clone = file_name.to_string();
                        info!("Parsing file: {}", file_path.display());
                        parse_futures.push(tokio::spawn(async move {
                            match parse_python_file(&file_path).await {
                                Ok(file_api) => Some(file_api),
                                Err(e) => {
                                    warn!("Failed to parse {}: {}", file_name_clone, e);
                                    None
                                }
                            }
                        }));
                    }
                }
            }
        }
        Ok(())
    }
    
    collect_python_files(source_dir, &mut parse_futures, &mut total_files)?;
    
    // Wait for all parsing to complete
    let results = futures::future::join_all(parse_futures).await;
    
    for result in results {
        if let Ok(Some(file_api)) = result {
            classes.extend(file_api.classes);
            enums.extend(file_api.enums);
            functions.extend(file_api.functions);
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
            extraction_timestamp: chrono::Utc::now().to_rfc3339(),
        },
    })
}

#[derive(Debug, Serialize, Deserialize)]
struct FileApi {
    classes: Vec<PythonClass>,
    enums: Vec<PythonEnum>,
    functions: Vec<PythonFunction>,
}

async fn parse_python_file(file_path: &Path) -> Result<FileApi> {
    let file_str = file_path.to_string_lossy().to_string();
    
    // Parse the Python file using tree-parser
    let parsed_file = tree_parser::parse_file(&file_str, tree_parser::Language::Python).await?;
    
    let mut classes = Vec::new();
    let mut enums = Vec::new();
    let mut functions = Vec::new();
    
    // Find all class definitions
    let class_constructs = tree_parser::search_by_node_type(&parsed_file, "class_definition", None);
    for class_construct in class_constructs {
        if let Some(_class_name) = &class_construct.name {
            let class = parse_class_definition(&parsed_file, &class_construct, file_path)?;
            classes.push(class);
        }
    }
    
    // Find all function definitions (not inside classes)
    let function_constructs = tree_parser::search_by_node_type(&parsed_file, "function_definition", None);
    for func_construct in function_constructs {
        if let Some(_func_name) = &func_construct.name {
            // Check if this function is not inside a class
            if !is_inside_class(&parsed_file, &func_construct) {
                let func = parse_function_definition(&parsed_file, &func_construct, file_path)?;
                functions.push(func);
            }
        }
    }
    
    // Find all enum definitions (Python enums are typically classes inheriting from Enum)
    // First check if Enum is imported in this file
    if inherits_from_enum(&parsed_file) {
        let all_class_constructs = tree_parser::search_by_node_type(&parsed_file, "class_definition", None);
        
        for class_construct in all_class_constructs {
            if let Some(_class_name) = &class_construct.name {
                // Check if it inherits from Enum
                if let Some(superclasses) = find_superclasses(&parsed_file, &class_construct) {
                    if superclasses.iter().any(|superclass| {
                        superclass == "Enum" || superclass.ends_with("Enum")
                    }) {
                        let enum_def = parse_enum_definition(&parsed_file, &class_construct, file_path)?;
                        enums.push(enum_def);
                    }
                }
            }
        }
    }
    
    Ok(FileApi {
        classes,
        enums,
        functions,
    })
}

fn parse_class_definition(
    parsed_file: &tree_parser::ParsedFile,
    class_construct: &tree_parser::CodeConstruct,
    file_path: &Path,
) -> Result<PythonClass> {
    let mut methods = Vec::new();
    let mut properties = Vec::new();
    let mut inherits = Vec::new();
    
    // Extract inheritance
    if let Some(superclasses) = find_superclasses(parsed_file, class_construct) {
        inherits = superclasses;
    }
    
    // Find methods inside this class - simpler approach
    let all_function_constructs = tree_parser::search_by_node_type(parsed_file, "function_definition", None);
    for func_construct in all_function_constructs {
        if is_within_construct(parsed_file, &func_construct, class_construct) {
            if let Some(_method_name) = &func_construct.name {
                let method = parse_method_definition(parsed_file, &func_construct)?;
                methods.push(method);
            }
        }
    }
    
    // Find properties (decorated with @property) - simpler approach
    let all_decorated_constructs = tree_parser::search_by_node_type(parsed_file, "decorated_definition", None);
    for decorated_construct in all_decorated_constructs {
        if is_within_construct(parsed_file, &decorated_construct, class_construct) {
            if is_property_decorator(parsed_file, &decorated_construct) {
                if let Some(_property_name) = &decorated_construct.name {
                    let property = parse_property_definition(parsed_file, &decorated_construct)?;
                    properties.push(property);
                }
            }
        }
    }
    
    let is_exception = inherits.contains(&"Exception".to_string());
    let is_abstract = inherits.contains(&"ABC".to_string()) || inherits.iter().any(|s| s.contains("Abstract"));
    
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
    parsed_file: &tree_parser::ParsedFile,
    method_construct: &tree_parser::CodeConstruct,
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
    parsed_file: &tree_parser::ParsedFile,
    func_construct: &tree_parser::CodeConstruct,
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
    parsed_file: &tree_parser::ParsedFile,
    enum_construct: &tree_parser::CodeConstruct,
    file_path: &Path,
) -> Result<PythonEnum> {
    let values = extract_enum_values(parsed_file, enum_construct);
    
    Ok(PythonEnum {
        name: enum_construct.name.clone().unwrap_or_default(),
        file_path: file_path.to_string_lossy().to_string(),
        docstring: extract_docstring(parsed_file, enum_construct),
        values,
    })
}

fn parse_property_definition(
    parsed_file: &tree_parser::ParsedFile,
    property_construct: &tree_parser::CodeConstruct,
) -> Result<PythonProperty> {
    Ok(PythonProperty {
        name: property_construct.name.clone().unwrap_or_default(),
        type_hint: extract_return_type(parsed_file, property_construct),
        docstring: extract_docstring(parsed_file, property_construct),
        is_readonly: !has_setter(parsed_file, property_construct),
    })
}

// Helper functions for extraction
fn extract_docstring(
    parsed_file: &tree_parser::ParsedFile,
    construct: &tree_parser::CodeConstruct,
) -> String {
    // Look for string literals within the construct
    let string_literals = tree_parser::search_by_node_type(parsed_file, "string_literal", None);
    
    for string_literal in string_literals {
        if is_within_construct(parsed_file, &string_literal, construct) {
            // Check if this is likely a docstring (first string literal in construct)
            if !string_literal.source_code.is_empty() {
                // Check if it's near the beginning of the construct
                if (string_literal.start_byte - construct.start_byte) < 300 {
                    // Clean up the string literal (remove quotes)
                    let mut content = string_literal.source_code.clone();
                    if content.starts_with('"') && content.ends_with('"') && content.len() > 2 {
                        content = content[1..content.len()-1].to_string();
                    } else if content.starts_with('\'') && content.ends_with('\'') && content.len() > 2 {
                        content = content[1..content.len()-1].to_string();
                    } else if content.starts_with("\"\"\"") && content.ends_with("\"\"\"") && content.len() > 6 {
                        content = content[3..content.len()-3].to_string();
                    } else if content.starts_with("'''") && content.ends_with("'''") && content.len() > 6 {
                        content = content[3..content.len()-3].to_string();
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

fn extract_parameters(
    _parsed_file: &tree_parser::ParsedFile,
    construct: &tree_parser::CodeConstruct,
) -> Vec<Parameter> {
    // Debug: Log what we're looking for
    eprintln!("DEBUG: Extracting parameters for function: {:?}", construct.name);
    eprintln!("DEBUG: Function construct spans bytes {} to {}", construct.start_byte, construct.end_byte);
    
    // Check if the tree-parser already extracted parameters in the metadata
    eprintln!("DEBUG: Metadata parameters: {:?}", construct.metadata.parameters);
    
    if !construct.metadata.parameters.is_empty() {
        eprintln!("DEBUG: Using parameters from metadata");
        // Convert tree-parser Parameter to our Parameter type
        return construct.metadata.parameters.iter().map(|p| Parameter {
            name: p.name.clone(),
            type_hint: p.param_type.clone().unwrap_or_else(|| "Any".to_string()),
            default_value: p.default_value.clone(),
        }).collect();
    }
    
    // If metadata doesn't have parameters, try a more direct approach
    // Look for parameters node within the function definition using tree-sitter queries
    let mut parameters = Vec::new();
    
    // Debug: Check children of this function construct
    eprintln!("DEBUG: Function construct has {} children", construct.children.len());
    for (i, child) in construct.children.iter().enumerate() {
        eprintln!("DEBUG: Child {}: {} ({:?})", i, child.node_type, child.name);
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
    eprintln!("DEBUG: Function source code: {}", source);
    
    // Look for the parameter list between parentheses
    if let Some(start) = source.find('(') {
        if let Some(end) = source.find(')') {
            let param_section = &source[start + 1..end];
            eprintln!("DEBUG: Parameter section: {}", param_section);
            
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
                            eprintln!("DEBUG: Adding parameter: {} with type: {}", final_name, type_hint);
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
    
    eprintln!("DEBUG: Final parameters: {:?}", parameters);
    parameters
}

fn is_parameter_identifier(
    parsed_file: &tree_parser::ParsedFile,
    identifier: &tree_parser::CodeConstruct,
    func_construct: &tree_parser::CodeConstruct,
) -> bool {
    // Look for parameters node within the function definition
    let param_nodes = tree_parser::search_by_node_type(parsed_file, "parameters", None);
    
    for param_node in param_nodes {
        if is_within_construct(parsed_file, &param_node, func_construct) {
            // Check if this identifier is within the parameters node
            return is_within_construct(parsed_file, identifier, &param_node);
        }
    }
    
    false
}

fn extract_type_hint_for_param(
    parsed_file: &tree_parser::ParsedFile,
    identifier: &tree_parser::CodeConstruct,
    func_construct: &tree_parser::CodeConstruct,
) -> String {
    // Look for type annotation nodes near this parameter
    let type_nodes = tree_parser::search_by_node_type(parsed_file, "type", None);
    
    for type_node in type_nodes {
        if is_within_construct(parsed_file, &type_node, func_construct) {
            // Check if this type node is close to the identifier (likely its type annotation)
            if (type_node.start_byte as i32 - identifier.end_byte as i32).abs() < 50 {
                if !type_node.source_code.is_empty() {
                    return type_node.source_code.clone();
                }
            }
        }
    }
    
    "Any".to_string()
}

fn extract_default_value_for_param(
    parsed_file: &tree_parser::ParsedFile,
    identifier: &tree_parser::CodeConstruct,
    func_construct: &tree_parser::CodeConstruct,
) -> Option<String> {
    // Look for default parameter nodes near this parameter
    let default_nodes = tree_parser::search_by_node_type(parsed_file, "default_parameter", None);
    
    for default_node in default_nodes {
        if is_within_construct(parsed_file, &default_node, func_construct) {
            // Check if this default node is close to the identifier
            if (default_node.start_byte as i32 - identifier.end_byte as i32).abs() < 100 {
                if !default_node.source_code.is_empty() {
                    return Some(default_node.source_code.clone());
                }
            }
        }
    }
    
    None
}

fn extract_signature(
    parsed_file: &tree_parser::ParsedFile,
    construct: &tree_parser::CodeConstruct,
) -> String {
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

fn extract_return_type(
    parsed_file: &tree_parser::ParsedFile,
    construct: &tree_parser::CodeConstruct,
) -> String {
    // Look for return type annotation
    let return_query = format!(
        "(function_definition
          return_type: (type) @return_type
        )"
    );
    
    if let Ok(return_constructs) = tree_parser::search_by_query(parsed_file, &return_query) {
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

fn is_async_function(
    parsed_file: &tree_parser::ParsedFile,
    construct: &tree_parser::CodeConstruct,
) -> bool {
    // Look for async keyword nodes within the function definition
    let async_nodes = tree_parser::search_by_node_type(parsed_file, "async", None);
    
    for async_node in async_nodes {
        if is_within_construct(parsed_file, &async_node, construct) {
            return true;
        }
    }
    
    false
}

fn is_static_method(
    parsed_file: &tree_parser::ParsedFile,
    construct: &tree_parser::CodeConstruct,
) -> bool {
    // Check if the method has @staticmethod decorator
    let decorators = extract_decorators(parsed_file, construct);
    decorators.contains(&"staticmethod".to_string())
}

fn extract_decorators(
    parsed_file: &tree_parser::ParsedFile,
    construct: &tree_parser::CodeConstruct,
) -> Vec<String> {
    let mut decorators = Vec::new();
    
    // Look for decorator nodes
    let decorator_nodes = tree_parser::search_by_node_type(parsed_file, "decorator", None);
    
    for decorator_node in decorator_nodes {
        if is_within_construct(parsed_file, &decorator_node, construct) {
            // Look for identifiers within this decorator
            let decorator_identifiers = tree_parser::search_by_node_type(parsed_file, "identifier", None);
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

fn is_inside_class(
    parsed_file: &tree_parser::ParsedFile,
    construct: &tree_parser::CodeConstruct,
) -> bool {
    // Look for parent class_definition
    let class_query = format!(
        "(class_definition
          body: (block
            (function_definition) @nested_func
          )
        )"
    );
    
    if let Ok(class_constructs) = tree_parser::search_by_query(parsed_file, &class_query) {
        for class_construct in class_constructs {
            if is_within_construct(parsed_file, construct, &class_construct) {
                return true;
            }
        }
    }
    
    false
}

fn find_superclasses(
    parsed_file: &tree_parser::ParsedFile,
    construct: &tree_parser::CodeConstruct,
) -> Option<Vec<String>> {
    let mut superclasses = Vec::new();
    
    // Look for argument_list in class definition
    let superclass_query = format!(
        "(class_definition
          superclasses: (argument_list
            (identifier) @superclass
          )
        )"
    );
    
    if let Ok(superclass_constructs) = tree_parser::search_by_query(parsed_file, &superclass_query) {
        for superclass_construct in superclass_constructs {
            if is_within_construct(parsed_file, &superclass_construct, construct) {
                if let Some(name) = &superclass_construct.name {
                    superclasses.push(name.clone());
                }
            }
        }
    }
    
    if superclasses.is_empty() {
        None
    } else {
        Some(superclasses)
    }
}

fn inherits_from_enum(
    parsed_file: &tree_parser::ParsedFile,
) -> bool {
    // Look for import statements that import Enum
    let import_nodes = tree_parser::search_by_node_type(parsed_file, "import_statement", None);
    
    for import_node in import_nodes {
        // Look for identifiers within import statements
        let import_identifiers = tree_parser::search_by_node_type(parsed_file, "identifier", None);
        for identifier in import_identifiers {
            if is_within_construct(parsed_file, &identifier, &import_node) {
                if let Some(name) = &identifier.name {
                    if name == "Enum" {
                        return true;
                    }
                }
            }
        }
    }
    
    // Also check for from imports
    let import_from_nodes = tree_parser::search_by_node_type(parsed_file, "import_from_statement", None);
    
    for import_from_node in import_from_nodes {
        // Look for identifiers within import from statements
        let import_identifiers = tree_parser::search_by_node_type(parsed_file, "identifier", None);
        for identifier in import_identifiers {
            if is_within_construct(parsed_file, &identifier, &import_from_node) {
                if let Some(name) = &identifier.name {
                    if name == "Enum" {
                        return true;
                    }
                }
            }
        }
    }
    
    false
}

fn is_property_decorator(
    parsed_file: &tree_parser::ParsedFile,
    construct: &tree_parser::CodeConstruct,
) -> bool {
    // Look for decorator nodes within the decorated definition
    let decorator_nodes = tree_parser::search_by_node_type(parsed_file, "decorator", None);
    
    for decorator_node in decorator_nodes {
        if is_within_construct(parsed_file, &decorator_node, construct) {
            // Look for identifiers within this decorator
            let decorator_identifiers = tree_parser::search_by_node_type(parsed_file, "identifier", None);
            for identifier in decorator_identifiers {
                if is_within_construct(parsed_file, &identifier, &decorator_node) {
                    if let Some(name) = &identifier.name {
                        if name == "property" {
                            return true;
                        }
                    }
                }
            }
        }
    }
    
    false
}

fn has_setter(
    parsed_file: &tree_parser::ParsedFile,
    construct: &tree_parser::CodeConstruct,
) -> bool {
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
        
        if let Ok(setter_constructs) = tree_parser::search_by_query(parsed_file, &setter_query) {
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
    parsed_file: &tree_parser::ParsedFile,
    construct: &tree_parser::CodeConstruct,
) -> Vec<EnumValue> {
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
    
    if let Ok(enum_value_constructs) = tree_parser::search_by_query(parsed_file, &enum_value_query) {
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
    
    values
}

fn extract_type_hint(
    parsed_file: &tree_parser::ParsedFile,
    construct: &tree_parser::CodeConstruct,
) -> String {
    // Look for type annotation
    let type_query = format!(
        "(type) @type_hint"
    );
    
    if let Ok(type_constructs) = tree_parser::search_by_query(parsed_file, &type_query) {
        for type_construct in type_constructs {
            if is_within_construct(parsed_file, &type_construct, construct) {
                if !type_construct.source_code.is_empty() {
                    return type_construct.source_code.clone();
                }
            }
        }
    }
    
    "Any".to_string()
}

fn extract_default_value(
    parsed_file: &tree_parser::ParsedFile,
    construct: &tree_parser::CodeConstruct,
) -> Option<String> {
    // Look for default parameter value
    let default_query = format!(
        "(default_parameter
          (identifier) @param_name
          (expression) @default_value
        )"
    );
    
    if let Ok(default_constructs) = tree_parser::search_by_query(parsed_file, &default_query) {
        for default_construct in default_constructs {
            if is_within_construct(parsed_file, &default_construct, construct) {
                if !default_construct.source_code.is_empty() {
                    return Some(default_construct.source_code.clone());
                }
            }
        }
    }
    
    None
}

fn is_within_construct(
    _parsed_file: &tree_parser::ParsedFile,
    inner: &tree_parser::CodeConstruct,
    outer: &tree_parser::CodeConstruct,
) -> bool {
    // Check if inner construct is within outer construct using position information
    let (inner_start, inner_end, outer_start, outer_end) = 
        (inner.start_byte, inner.end_byte, outer.start_byte, outer.end_byte);
    
    inner_start >= outer_start && inner_end <= outer_end
}

// Query and export functions
fn execute_query(api: &PythonApi, _query: &str) -> Result<()> {
    let cli = Cli::parse();
    
    let mut filtered_classes = api.classes.clone();
    
    // Apply class filter
    if let Some(class_filter) = &cli.class {
        let class_names: Vec<&str> = class_filter.split(',').map(|s| s.trim()).collect();
        filtered_classes.retain(|class| {
            class_names.iter().any(|&name| 
                class.name.to_lowercase().contains(&name.to_lowercase())
            )
        });
    }
    
    // Apply method filter
    if let Some(method_filter) = &cli.method {
        for class in &mut filtered_classes {
            class.methods.retain(|method| 
                method.name.to_lowercase().contains(&method_filter.to_lowercase())
            );
        }
        // Also filter standalone functions
        let mut filtered_functions = api.functions.clone();
        filtered_functions.retain(|func| 
            func.name.to_lowercase().contains(&method_filter.to_lowercase())
        );
        
        // Create filtered API
        let filtered_api = PythonApi {
            classes: filtered_classes,
            enums: api.enums.clone(),
            functions: filtered_functions,
            metadata: api.metadata.clone(),
        };
        
        output_filtered_api(&filtered_api, &cli.format)?;
        return Ok(());
    }
    
    // Apply parameter filter
    if let Some(param_filter) = &cli.parameter {
        for class in &mut filtered_classes {
            class.methods.retain(|method| 
                method.parameters.iter().any(|param| 
                    param.name.to_lowercase().contains(&param_filter.to_lowercase())
                )
            );
        }
        // Remove classes with no methods after filtering
        filtered_classes.retain(|class| !class.methods.is_empty());
    }
    
    // Create filtered API
    let filtered_api = PythonApi {
        classes: filtered_classes,
        enums: api.enums.clone(),
        functions: api.functions.clone(), // Keep all functions unless specifically filtered
        metadata: api.metadata.clone(),
    };
    
    output_filtered_api(&filtered_api, &cli.format)?;
    Ok(())
}

fn output_filtered_api(api: &PythonApi, format: &str) -> Result<()> {
    match format {
        "json" => {
            let json = serde_json::to_string_pretty(api)?;
            println!("{}", json);
        }
        "csv" => {
            export_to_csv(api, "query_output.csv")?;
        }
        "markdown" => {
            export_to_markdown(api, "query_output.md")?;
        }
        _ => {
            return Err(anyhow::anyhow!("Unsupported format: {}", format));
        }
    }
    Ok(())
}

fn export_to_csv(api: &PythonApi, output_path: &str) -> Result<()> {
    use std::io::Write;
    
    let mut file = std::fs::File::create(output_path)?;
    
    // Write CSV header
    writeln!(file, "Type,Name,File,Method,Parameters,IsAsync,IsStatic")?;
    
    // Export classes and methods
    for class in &api.classes {
        for method in &class.methods {
            let params: Vec<String> = method.parameters.iter()
                .map(|p| format!("{}: {}", p.name, p.type_hint))
                .collect();
            writeln!(file, "Class,{},\"{}\",{},\"{}\",{},{}",
                class.name,
                class.file_path,
                method.name,
                params.join("; "),
                method.is_async,
                method.is_static
            )?;
        }
    }
    
    // Export functions
    for func in &api.functions {
        let params: Vec<String> = func.parameters.iter()
            .map(|p| format!("{}: {}", p.name, p.type_hint))
            .collect();
        writeln!(file, "Function,{},\"{}\",,\"{}\",{},",
            func.name,
            func.file_path,
            params.join("; "),
            func.is_async
        )?;
    }
    
    Ok(())
}

fn export_to_markdown(api: &PythonApi, output_path: &str) -> Result<()> {
    use std::io::Write;
    
    let mut file = std::fs::File::create(output_path)?;
    
    writeln!(file, "# iTerm2 Python API Reference\n")?;
    writeln!(file, "Generated on: {}\n", api.metadata.extraction_timestamp)?;
    writeln!(file, "- **Total Files**: {}", api.metadata.total_files)?;
    writeln!(file, "- **Total Classes**: {}", api.metadata.total_classes)?;
    writeln!(file, "- **Total Functions**: {}", api.metadata.total_functions)?;
    writeln!(file, "- **Total Enums**: {}\n", api.metadata.total_enums)?;
    
    // Export classes
    for class in &api.classes {
        writeln!(file, "## Class: `{}`\n", class.name)?;
        writeln!(file, "**File**: `{}`", class.file_path)?;
        if !class.inherits.is_empty() {
            writeln!(file, "**Inherits**: {}", class.inherits.join(", "))?;
        }
        writeln!(file, "**Line**: {}", class.line_number.unwrap_or(0))?;
        writeln!(file, "**Methods**: {}\n", class.methods.len())?;
        
        if !class.methods.is_empty() {
            writeln!(file, "### Methods\n")?;
            for method in &class.methods {
                writeln!(file, "#### `{}`", method.signature)?;
                if method.is_async {
                    writeln!(file, "- **Async**: Yes")?;
                }
                if method.is_static {
                    writeln!(file, "- **Static**: Yes")?;
                }
                if !method.parameters.is_empty() {
                    writeln!(file, "- **Parameters**:")?;
                    for param in &method.parameters {
                        writeln!(file, "  - `{}`: `{}`", param.name, param.type_hint)?;
                    }
                }
                if !method.returns.is_empty() && method.returns != "Any" {
                    writeln!(file, "- **Returns**: `{}`", method.returns)?;
                }
                writeln!(file)?;
            }
        }
    }
    
    // Export functions
    if !api.functions.is_empty() {
        writeln!(file, "## Functions\n")?;
        for func in &api.functions {
            writeln!(file, "### `{}`\n", func.signature)?;
            writeln!(file, "**File**: `{}`", func.file_path)?;
            if func.is_async {
                writeln!(file, "- **Async**: Yes")?;
            }
            if !func.parameters.is_empty() {
                writeln!(file, "- **Parameters**:")?;
                for param in &func.parameters {
                    writeln!(file, "  - `{}`: `{}`", param.name, param.type_hint)?;
                }
            }
            if !func.returns.is_empty() && func.returns != "Any" {
                writeln!(file, "- **Returns**: `{}`", func.returns)?;
            }
            writeln!(file)?;
        }
    }
    
    Ok(())
}

fn generate_progress_report(api: &PythonApi) -> Result<()> {
    use std::io::Write;
    
    let mut file = std::fs::File::create("PROGRESS_UPDATE.md")?;
    
    writeln!(file, "# iTerm2 Python API Analysis Report\n")?;
    writeln!(file, "Generated on: {}\n", api.metadata.extraction_timestamp)?;
    
    // Key classes analysis
    let key_classes = ["App", "Window", "Tab", "Session"];
    writeln!(file, "## Key Classes Analysis\n")?;
    
    for class_name in &key_classes {
        if let Some(class) = api.classes.iter().find(|c| c.name == *class_name) {
            writeln!(file, "### `{}`\n", class.name)?;
            writeln!(file, "- **Total Methods**: {}", class.methods.len())?;
            writeln!(file, "- **File**: `{}`", class.file_path)?;
            
            // Method analysis
            let async_methods = class.methods.iter().filter(|m| m.is_async).count();
            let static_methods = class.methods.iter().filter(|m| m.is_static).count();
            let methods_with_params = class.methods.iter().filter(|m| !m.parameters.is_empty()).count();
            
            writeln!(file, "- **Async Methods**: {}", async_methods)?;
            writeln!(file, "- **Static Methods**: {}", static_methods)?;
            writeln!(file, "- **Methods with Parameters**: {}", methods_with_params)?;
            
            // Sample methods
            if !class.methods.is_empty() {
                writeln!(file, "- **Sample Methods**:")?;
                for method in class.methods.iter().take(3) {
                    writeln!(file, "  - `{}`", method.signature)?;
                }
            }
            writeln!(file)?;
        }
    }
    
    // Parameter frequency analysis
    writeln!(file, "## Parameter Frequency Analysis\n")?;
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
    
    writeln!(file, "| Parameter | Count |")?;
    writeln!(file, "|-----------|-------|")?;
    for (param, count) in sorted_params.iter().take(10) {
        writeln!(file, "| `{}` | {} |", param, count)?;
    }
    
    // Type analysis
    writeln!(file, "\n## Type Hint Analysis\n")?;
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
    
    writeln!(file, "| Type | Count |")?;
    writeln!(file, "|------|-------|")?;
    for (type_hint, count) in sorted_types.iter().take(10) {
        writeln!(file, "`{}` | {} |", type_hint, count)?;
    }
    
    info!("Progress report generated: PROGRESS_UPDATE.md");
    Ok(())
}

fn show_summary(api: &PythonApi) {
    println!("📊 iTerm2 Python API Summary");
    println!("═══════════════════════════");
    println!("📁 Total Files: {}", api.metadata.total_files);
    println!("🏗️  Total Classes: {}", api.metadata.total_classes);
    println!("⚙️  Total Functions: {}", api.metadata.total_functions);
    println!("🔢 Total Enums: {}", api.metadata.total_enums);
    println!();
    
    // Key classes
    let key_classes = ["App", "Window", "Tab", "Session"];
    println!("🎯 Key Classes:");
    for class_name in &key_classes {
        if let Some(class) = api.classes.iter().find(|c| c.name == *class_name) {
            println!("  • {}: {} methods", class.name, class.methods.len());
        }
    }
    println!();
    
    // Method statistics
    let total_methods: usize = api.classes.iter().map(|c| c.methods.len()).sum();
    let async_methods: usize = api.classes.iter()
        .flat_map(|c| c.methods.iter())
        .filter(|m| m.is_async)
        .count();
    
    println!("📈 Method Statistics:");
    println!("  • Total Methods: {}", total_methods);
    println!("  • Async Methods: {}", async_methods);
    println!("  • Sync Methods: {}", total_methods - async_methods);
    println!();
    
    println!("💡 Use --help to see query and export options");
}
